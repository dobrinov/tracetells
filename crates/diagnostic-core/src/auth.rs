//! Users, roles, sessions, invites, API tokens and two-step verification (TOTP).
//!
//! Secrets the server hands out (session cookies, invite links, API tokens,
//! recovery codes) are stored only as SHA-256 hashes, so a copy of the
//! database can't be used to sign in. Passwords use Argon2id.

use std::fmt;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use data_encoding::{BASE32_NOPAD, BASE64URL_NOPAD, HEXLOWER};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use subtle::ConstantTimeEq;

use crate::util::{DAY_MS, now_ms};

pub const SESSION_DAYS: i64 = 30;
pub const INVITE_DAYS: i64 = 7;
pub const MIN_PASSWORD_LEN: usize = 10;
const TOTP_STEP_SECS: i64 = 30;
const RECOVERY_CODES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Member,
    Admin,
    Owner,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "viewer" => Self::Viewer,
            "member" => Self::Member,
            "admin" => Self::Admin,
            "owner" => Self::Owner,
            other => bail!("unknown role {other:?} (expected viewer, member, admin or owner)"),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub totp_enabled: bool,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
}

// ---- primitives ------------------------------------------------------------

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("the OS random number generator is available");
    b
}

/// A URL-safe random secret with `bytes` of entropy.
pub fn random_token(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::getrandom(&mut b).expect("the OS random number generator is available");
    BASE64URL_NOPAD.encode(&b)
}

pub fn sha256_hex(s: &str) -> String {
    HEXLOWER.encode(&Sha256::digest(s.as_bytes()))
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::encode_b64(&random_bytes::<16>()).map_err(|e| anyhow!("salt: {e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("hashing password: {e}"))?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

/// Spends about as long as a real check, so unknown emails can't be told
/// apart from wrong passwords by timing.
pub fn waste_password_check(password: &str) {
    static DUMMY: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| hash_password("not-a-real-password").expect("hashing works"));
    let _ = verify_password(password, &DUMMY);
}

pub fn check_new_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        bail!("use at least {MIN_PASSWORD_LEN} characters");
    }
    if password.len() > 1024 {
        bail!("that password is too long");
    }
    Ok(())
}

fn normalize_email(email: &str) -> Result<String> {
    let e = email.trim().to_lowercase();
    let valid = e.len() <= 254
        && e.split_once('@').is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.') && !domain.starts_with('.'))
        && !e.contains(char::is_whitespace);
    if !valid {
        bail!("enter a valid email address");
    }
    Ok(e)
}

// ---- TOTP (RFC 6238, SHA-1, 6 digits, 30 s) ---------------------------------

pub fn new_totp_secret() -> String {
    BASE32_NOPAD.encode(&random_bytes::<20>())
}

fn totp_at(secret: &str, step: i64) -> Result<u32> {
    let key = BASE32_NOPAD.decode(secret.as_bytes()).context("invalid TOTP secret")?;
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(&key).map_err(|e| anyhow!("{e}"))?;
    mac.update(&(step as u64).to_be_bytes());
    let h = mac.finalize().into_bytes();
    let offset = (h[19] & 0x0f) as usize;
    let bin = u32::from_be_bytes([h[offset] & 0x7f, h[offset + 1], h[offset + 2], h[offset + 3]]);
    Ok(bin % 1_000_000)
}

/// The time step a code matches, allowing one step of clock drift either way.
fn totp_match(secret: &str, code: &str, now_ms: i64) -> Option<i64> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        return None;
    }
    let step = now_ms / 1000 / TOTP_STEP_SECS;
    (step - 1..=step + 1).find(|s| totp_at(secret, *s).is_ok_and(|c| constant_time_eq(&format!("{c:06}"), &code)))
}

pub fn otpauth_uri(issuer: &str, account: &str, secret: &str) -> String {
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~@".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    format!(
        "otpauth://totp/{}:{}?secret={secret}&issuer={}&algorithm=SHA1&digits=6&period={TOTP_STEP_SECS}",
        enc(issuer),
        enc(account),
        enc(issuer)
    )
}

pub fn qr_svg(data: &str) -> Result<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).map_err(|e| anyhow!("QR code: {e}"))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#111111"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build())
}

fn new_recovery_codes() -> Vec<String> {
    (0..RECOVERY_CODES)
        .map(|_| {
            let s = BASE32_NOPAD.encode(&random_bytes::<5>()).to_lowercase();
            format!("{}-{}", &s[..4], &s[4..8])
        })
        .collect()
}

fn normalize_recovery(code: &str) -> String {
    let c: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    if c.len() == 8 { format!("{}-{}", &c[..4], &c[4..]) } else { c }
}

// ---- users -----------------------------------------------------------------

type UserRow = (i64, String, String, String, Option<i64>, i64, Option<i64>);
const USER_COLS: &str = "id, email, name, role, totp_enabled_at, created_at, last_login_at";

fn user_from((id, email, name, role, totp, created_at, last_login_at): UserRow) -> Result<User> {
    Ok(User { id, email, name, role: role.parse()?, totp_enabled: totp.is_some(), created_at, last_login_at })
}

pub async fn user_count(pool: &SqlitePool, project_id: i64) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE project_id = ?").bind(project_id).fetch_one(pool).await?)
}

pub async fn create_user(pool: &SqlitePool, project_id: i64, email: &str, name: &str, password: &str, role: Role) -> Result<User> {
    let email = normalize_email(email)?;
    check_new_password(password)?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE project_id = ? AND email = ?")
        .bind(project_id)
        .bind(&email)
        .fetch_optional(pool)
        .await?;
    if exists.is_some() {
        bail!("{email} already has an account");
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (project_id, email, name, password_hash, role, created_at) VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(project_id)
    .bind(&email)
    .bind(name.trim())
    .bind(hash_password(password)?)
    .bind(role.as_str())
    .bind(now_ms())
    .fetch_one(pool)
    .await?;
    get_user(pool, project_id, id).await?.context("user was just created")
}

pub async fn get_user(pool: &SqlitePool, project_id: i64, id: i64) -> Result<Option<User>> {
    let row: Option<UserRow> = sqlx::query_as(&format!("SELECT {USER_COLS} FROM users WHERE project_id = ? AND id = ?"))
        .bind(project_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(user_from).transpose()
}

pub async fn find_user(pool: &SqlitePool, project_id: i64, email: &str) -> Result<Option<User>> {
    let Ok(email) = normalize_email(email) else { return Ok(None) };
    let row: Option<UserRow> = sqlx::query_as(&format!("SELECT {USER_COLS} FROM users WHERE project_id = ? AND email = ?"))
        .bind(project_id)
        .bind(email)
        .fetch_optional(pool)
        .await?;
    row.map(user_from).transpose()
}

pub async fn list_users(pool: &SqlitePool, project_id: i64) -> Result<Vec<User>> {
    let rows: Vec<UserRow> = sqlx::query_as(&format!(
        "SELECT {USER_COLS} FROM users WHERE project_id = ? ORDER BY CASE role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 WHEN 'member' THEN 2 ELSE 3 END, email"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(user_from).collect()
}

/// Checks email and password. Spends the same time whether or not the user exists.
pub async fn authenticate(pool: &SqlitePool, project_id: i64, email: &str, password: &str) -> Result<Option<User>> {
    let row: Option<(i64, String)> = match normalize_email(email) {
        Ok(e) => sqlx::query_as("SELECT id, password_hash FROM users WHERE project_id = ? AND email = ?")
            .bind(project_id)
            .bind(e)
            .fetch_optional(pool)
            .await?,
        Err(_) => None,
    };
    let Some((id, hash)) = row else {
        waste_password_check(password);
        return Ok(None);
    };
    if !verify_password(password, &hash) {
        return Ok(None);
    }
    get_user(pool, project_id, id).await
}

pub async fn set_password(pool: &SqlitePool, user_id: i64, password: &str) -> Result<()> {
    check_new_password(password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash_password(password)?).bind(user_id).execute(pool).await?;
    Ok(())
}

pub async fn password_matches(pool: &SqlitePool, user_id: i64, password: &str) -> Result<bool> {
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?").bind(user_id).fetch_one(pool).await?;
    Ok(verify_password(password, &hash))
}

pub async fn set_role(pool: &SqlitePool, project_id: i64, user_id: i64, role: Role) -> Result<()> {
    if role == Role::Owner {
        bail!("there is one owner per workspace");
    }
    let current = get_user(pool, project_id, user_id).await?.context("no such user")?;
    if current.role == Role::Owner {
        bail!("the owner's role can't be changed");
    }
    sqlx::query("UPDATE users SET role = ? WHERE id = ?").bind(role.as_str()).bind(user_id).execute(pool).await?;
    Ok(())
}

pub async fn remove_user(pool: &SqlitePool, project_id: i64, user_id: i64) -> Result<User> {
    let user = get_user(pool, project_id, user_id).await?.context("no such user")?;
    if user.role == Role::Owner {
        bail!("the owner can't be removed");
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE api_tokens SET revoked_at = ? WHERE created_by = ? AND revoked_at IS NULL")
        .bind(now_ms())
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for sql in [
        "DELETE FROM auth_sessions WHERE user_id = ?",
        "DELETE FROM recovery_codes WHERE user_id = ?",
        "DELETE FROM users WHERE id = ?",
    ] {
        sqlx::query(sql).bind(user_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(user)
}

// ---- sessions --------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Session {
    pub token_hash: String,
    pub user: User,
    /// Second factor passed (or not needed).
    pub mfa_ok: bool,
}

/// Starts a session and returns the cookie value. `mfa_ok` is false while a
/// second factor is still due.
pub async fn create_session(pool: &SqlitePool, user_id: i64, mfa_ok: bool, user_agent: Option<&str>) -> Result<String> {
    let token = random_token(32);
    let now = now_ms();
    sqlx::query(
        "INSERT INTO auth_sessions (token_hash, user_id, created_at, expires_at, last_seen_at, mfa_ok, user_agent) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(sha256_hex(&token))
    .bind(user_id)
    .bind(now)
    .bind(now + SESSION_DAYS * DAY_MS)
    .bind(now)
    .bind(mfa_ok)
    .bind(user_agent.map(|u| crate::util::truncate(u, 300)))
    .execute(pool)
    .await?;
    if mfa_ok {
        sqlx::query("UPDATE users SET last_login_at = ? WHERE id = ?").bind(now).bind(user_id).execute(pool).await?;
    }
    Ok(token)
}

pub async fn session(pool: &SqlitePool, project_id: i64, token: &str) -> Result<Option<Session>> {
    let token_hash = sha256_hex(token);
    let row: Option<(i64, bool, i64, i64)> = sqlx::query_as(
        "SELECT s.user_id, s.mfa_ok, s.expires_at, s.last_seen_at FROM auth_sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ? AND u.project_id = ?",
    )
    .bind(&token_hash)
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    let Some((user_id, mfa_ok, expires_at, last_seen_at)) = row else { return Ok(None) };
    let now = now_ms();
    if expires_at <= now {
        end_session(pool, token).await?;
        return Ok(None);
    }
    if now - last_seen_at > 60_000 {
        sqlx::query("UPDATE auth_sessions SET last_seen_at = ? WHERE token_hash = ?").bind(now).bind(&token_hash).execute(pool).await?;
    }
    let Some(user) = get_user(pool, project_id, user_id).await? else { return Ok(None) };
    Ok(Some(Session { token_hash, user, mfa_ok }))
}

pub async fn mark_mfa_passed(pool: &SqlitePool, s: &Session) -> Result<()> {
    sqlx::query("UPDATE auth_sessions SET mfa_ok = 1 WHERE token_hash = ?").bind(&s.token_hash).execute(pool).await?;
    sqlx::query("UPDATE users SET last_login_at = ? WHERE id = ?").bind(now_ms()).bind(s.user.id).execute(pool).await?;
    Ok(())
}

pub async fn end_session(pool: &SqlitePool, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM auth_sessions WHERE token_hash = ?").bind(sha256_hex(token)).execute(pool).await?;
    Ok(())
}

/// Signs a user out everywhere, optionally keeping the current session.
pub async fn end_user_sessions(pool: &SqlitePool, user_id: i64, keep_token_hash: Option<&str>) -> Result<()> {
    sqlx::query("DELETE FROM auth_sessions WHERE user_id = ? AND token_hash <> ?")
        .bind(user_id)
        .bind(keep_token_hash.unwrap_or(""))
        .execute(pool)
        .await?;
    Ok(())
}

// ---- two-step verification ----------------------------------------------------

pub struct Enrollment {
    pub secret: String,
    pub uri: String,
    pub qr_svg: String,
}

/// Generates a new secret, kept pending until the user proves they can
/// produce codes with it.
pub async fn begin_totp(pool: &SqlitePool, user: &User, issuer: &str) -> Result<Enrollment> {
    let secret = new_totp_secret();
    sqlx::query("UPDATE users SET totp_pending_secret = ? WHERE id = ?").bind(&secret).bind(user.id).execute(pool).await?;
    let uri = otpauth_uri(issuer, &user.email, &secret);
    Ok(Enrollment { qr_svg: qr_svg(&uri)?, uri, secret })
}

/// Activates the pending secret if `code` matches it, and returns fresh recovery codes.
pub async fn confirm_totp(pool: &SqlitePool, user_id: i64, code: &str) -> Result<Vec<String>> {
    let pending: Option<String> = sqlx::query_scalar("SELECT totp_pending_secret FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    let secret = pending.context("start setting up the authenticator app first")?;
    let step = totp_match(&secret, code, now_ms()).context("that code doesn't match; check the time on your phone and try the newest code")?;
    sqlx::query(
        "UPDATE users SET totp_secret = ?, totp_pending_secret = NULL, totp_enabled_at = ?, totp_last_step = ? WHERE id = ?",
    )
    .bind(&secret)
    .bind(now_ms())
    .bind(step)
    .bind(user_id)
    .execute(pool)
    .await?;
    replace_recovery_codes(pool, user_id).await
}

pub async fn replace_recovery_codes(pool: &SqlitePool, user_id: i64) -> Result<Vec<String>> {
    let codes = new_recovery_codes();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?").bind(user_id).execute(&mut *tx).await?;
    for c in &codes {
        sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?, ?)").bind(user_id).bind(sha256_hex(c)).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(codes)
}

pub async fn recovery_codes_left(pool: &SqlitePool, user_id: i64) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM recovery_codes WHERE user_id = ? AND used_at IS NULL")
        .bind(user_id)
        .fetch_one(pool)
        .await?)
}

pub async fn disable_totp(pool: &SqlitePool, user_id: i64) -> Result<()> {
    sqlx::query("UPDATE users SET totp_secret = NULL, totp_pending_secret = NULL, totp_enabled_at = NULL, totp_last_step = NULL WHERE id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?").bind(user_id).execute(pool).await?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum SecondFactor {
    Totp,
    RecoveryCode,
}

/// Checks an authenticator code (each code works once) or an unused recovery code.
pub async fn verify_second_factor(pool: &SqlitePool, user_id: i64, code: &str) -> Result<Option<SecondFactor>> {
    let (secret, last_step): (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT totp_secret, totp_last_step FROM users WHERE id = ?").bind(user_id).fetch_one(pool).await?;
    let Some(secret) = secret else { return Ok(None) };
    if let Some(step) = totp_match(&secret, code, now_ms()) {
        if last_step.is_some_and(|l| step <= l) {
            return Ok(None); // replayed code
        }
        sqlx::query("UPDATE users SET totp_last_step = ? WHERE id = ?").bind(step).bind(user_id).execute(pool).await?;
        return Ok(Some(SecondFactor::Totp));
    }
    let used = sqlx::query("UPDATE recovery_codes SET used_at = ? WHERE user_id = ? AND code_hash = ? AND used_at IS NULL")
        .bind(now_ms())
        .bind(user_id)
        .bind(sha256_hex(&normalize_recovery(code)))
        .execute(pool)
        .await?;
    Ok((used.rows_affected() == 1).then_some(SecondFactor::RecoveryCode))
}

pub async fn require_mfa(pool: &SqlitePool, project_id: i64) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT require_mfa FROM projects WHERE id = ?").bind(project_id).fetch_one(pool).await?)
}

pub async fn set_require_mfa(pool: &SqlitePool, project_id: i64, on: bool) -> Result<()> {
    sqlx::query("UPDATE projects SET require_mfa = ? WHERE id = ?").bind(on).bind(project_id).execute(pool).await?;
    Ok(())
}

// ---- invites -------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Invite {
    pub id: i64,
    pub email: String,
    pub role: Role,
    pub created_at: i64,
    pub expires_at: i64,
}

/// Creates an invite and returns it with the secret for its link.
pub async fn create_invite(pool: &SqlitePool, project_id: i64, email: &str, role: Role, invited_by: i64) -> Result<(Invite, String)> {
    let email = normalize_email(email)?;
    if role == Role::Owner {
        bail!("invite people as admin, member or viewer");
    }
    if find_user(pool, project_id, &email).await?.is_some() {
        bail!("{email} already has an account");
    }
    let token = random_token(24);
    let now = now_ms();
    sqlx::query("DELETE FROM invites WHERE project_id = ? AND email = ? AND accepted_at IS NULL").bind(project_id).bind(&email).execute(pool).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO invites (project_id, token_hash, email, role, invited_by, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(project_id)
    .bind(sha256_hex(&token))
    .bind(&email)
    .bind(role.as_str())
    .bind(invited_by)
    .bind(now)
    .bind(now + INVITE_DAYS * DAY_MS)
    .fetch_one(pool)
    .await?;
    Ok((Invite { id, email, role, created_at: now, expires_at: now + INVITE_DAYS * DAY_MS }, token))
}

pub async fn pending_invites(pool: &SqlitePool, project_id: i64) -> Result<Vec<Invite>> {
    let rows: Vec<(i64, String, String, i64, i64)> = sqlx::query_as(
        "SELECT id, email, role, created_at, expires_at FROM invites WHERE project_id = ? AND accepted_at IS NULL AND expires_at > ? ORDER BY created_at DESC",
    )
    .bind(project_id)
    .bind(now_ms())
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(id, email, role, created_at, expires_at)| Ok(Invite { id, email, role: role.parse()?, created_at, expires_at }))
        .collect()
}

pub async fn revoke_invite(pool: &SqlitePool, project_id: i64, id: i64) -> Result<bool> {
    let r = sqlx::query("DELETE FROM invites WHERE project_id = ? AND id = ? AND accepted_at IS NULL").bind(project_id).bind(id).execute(pool).await?;
    Ok(r.rows_affected() > 0)
}

pub async fn find_invite(pool: &SqlitePool, project_id: i64, token: &str) -> Result<Option<Invite>> {
    let row: Option<(i64, String, String, i64, i64)> = sqlx::query_as(
        "SELECT id, email, role, created_at, expires_at FROM invites
         WHERE project_id = ? AND token_hash = ? AND accepted_at IS NULL AND expires_at > ?",
    )
    .bind(project_id)
    .bind(sha256_hex(token))
    .bind(now_ms())
    .fetch_optional(pool)
    .await?;
    row.map(|(id, email, role, created_at, expires_at)| Ok(Invite { id, email, role: role.parse()?, created_at, expires_at })).transpose()
}

pub async fn accept_invite(pool: &SqlitePool, project_id: i64, token: &str, name: &str, password: &str) -> Result<User> {
    let invite = find_invite(pool, project_id, token).await?.context("this invite link has expired or was already used")?;
    let user = create_user(pool, project_id, &invite.email, name, password, invite.role).await?;
    sqlx::query("UPDATE invites SET accepted_at = ? WHERE id = ?").bind(now_ms()).bind(invite.id).execute(pool).await?;
    Ok(user)
}

// ---- API tokens ------------------------------------------------------------------
//
// Tokens are personal: each belongs to the person who created it and acts with
// that person's current role (or as a viewer, if created read-only). Changing
// someone's role changes what their tokens can do; removing them revokes them.

#[derive(Debug, Serialize)]
pub struct ApiToken {
    pub id: i64,
    pub name: String,
    /// First characters, so people can tell tokens apart.
    pub prefix: String,
    pub read_only: bool,
    pub owner_id: i64,
    pub owner_email: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// A valid token and who it acts for.
#[derive(Debug, Clone)]
pub struct TokenAuth {
    pub id: i64,
    pub name: String,
    pub read_only: bool,
    pub user: User,
}

impl TokenAuth {
    /// The owner's role, or viewer for read-only tokens.
    pub fn role(&self) -> Role {
        if self.read_only { Role::Viewer } else { self.user.role }
    }
}

pub async fn create_api_token(pool: &SqlitePool, project_id: i64, owner: &User, name: &str, read_only: bool) -> Result<(ApiToken, String)> {
    let name = name.trim();
    if name.is_empty() {
        bail!("give the token a name, e.g. \"Claude Code\" or \"GitHub Actions\"");
    }
    let token = format!("dgt_{}", random_token(32));
    let prefix = token[..10].to_string();
    let now = now_ms();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO api_tokens (project_id, name, token_hash, prefix, created_by, read_only, created_at) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(project_id)
    .bind(name)
    .bind(sha256_hex(&token))
    .bind(&prefix)
    .bind(owner.id)
    .bind(read_only)
    .bind(now)
    .fetch_one(pool)
    .await?;
    let t = ApiToken {
        id,
        name: name.to_string(),
        prefix,
        read_only,
        owner_id: owner.id,
        owner_email: owner.email.clone(),
        created_at: now,
        last_used_at: None,
    };
    Ok((t, token))
}

/// Active tokens: one person's (`owner`), or everyone's for admins (`None`).
pub async fn list_api_tokens(pool: &SqlitePool, project_id: i64, owner: Option<i64>) -> Result<Vec<ApiToken>> {
    let rows: Vec<(i64, String, String, bool, i64, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT t.id, t.name, t.prefix, t.read_only, u.id, u.email, t.created_at, t.last_used_at
         FROM api_tokens t JOIN users u ON u.id = t.created_by
         WHERE t.project_id = ? AND t.revoked_at IS NULL AND (?2 IS NULL OR u.id = ?2)
         ORDER BY t.created_at DESC",
    )
    .bind(project_id)
    .bind(owner)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, prefix, read_only, owner_id, owner_email, created_at, last_used_at)| ApiToken {
            id,
            name,
            prefix,
            read_only,
            owner_id,
            owner_email,
            created_at,
            last_used_at,
        })
        .collect())
}

/// Revokes a token; with `owner`, only if it belongs to that person.
pub async fn revoke_api_token(pool: &SqlitePool, project_id: i64, id: i64, owner: Option<i64>) -> Result<bool> {
    let r = sqlx::query(
        "UPDATE api_tokens SET revoked_at = ?1 WHERE project_id = ?2 AND id = ?3 AND revoked_at IS NULL AND (?4 IS NULL OR created_by = ?4)",
    )
    .bind(now_ms())
    .bind(project_id)
    .bind(id)
    .bind(owner)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// The token and its owner, if the token is valid and its owner still has an account.
pub async fn verify_api_token(pool: &SqlitePool, project_id: i64, token: &str) -> Result<Option<TokenAuth>> {
    if !token.starts_with("dgt_") {
        return Ok(None);
    }
    let row: Option<(i64, String, bool, i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, name, read_only, created_by, last_used_at FROM api_tokens WHERE project_id = ? AND token_hash = ? AND revoked_at IS NULL",
    )
    .bind(project_id)
    .bind(sha256_hex(token))
    .fetch_optional(pool)
    .await?;
    let Some((id, name, read_only, owner, last_used)) = row else { return Ok(None) };
    let Some(user) = get_user(pool, project_id, owner).await? else { return Ok(None) };
    let now = now_ms();
    if last_used.is_none_or(|t| now - t > 60_000) {
        sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?").bind(now).bind(id).execute(pool).await?;
    }
    Ok(Some(TokenAuth { id, name, read_only, user }))
}

// ---- audit log -------------------------------------------------------------------

/// Records who changed what: team, security settings, tokens and privacy rules.
pub async fn audit(pool: &SqlitePool, project_id: i64, actor: &str, action: &str, detail: &str) -> Result<()> {
    sqlx::query("INSERT INTO audit_log (project_id, actor, action, detail, at) VALUES (?, ?, ?, ?, ?)")
        .bind(project_id)
        .bind(actor)
        .bind(action)
        .bind(detail)
        .bind(now_ms())
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub actor: String,
    pub action: String,
    pub detail: String,
    pub at: i64,
}

pub async fn audit_log(pool: &SqlitePool, project_id: i64, limit: i64) -> Result<Vec<AuditEntry>> {
    let rows: Vec<(String, String, String, i64)> =
        sqlx::query_as("SELECT actor, action, detail, at FROM audit_log WHERE project_id = ? ORDER BY id DESC LIMIT ?")
            .bind(project_id)
            .bind(limit)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(actor, action, detail, at)| AuditEntry { actor, action, detail, at }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_matches_rfc6238_test_vectors() {
        // RFC 6238 appendix B, SHA-1 key "12345678901234567890", last 6 digits.
        let secret = BASE32_NOPAD.encode(b"12345678901234567890");
        for (t, code) in [(59, 287082), (1111111109, 81804), (1234567890, 5924), (2000000000, 279037)] {
            assert_eq!(totp_at(&secret, t / 30).unwrap(), code, "t={t}");
        }
        assert_eq!(totp_match(&secret, "287 082", 59_000), Some(1));
        assert_eq!(totp_match(&secret, "287082", 59_000 + 3 * 30_000), None, "too old");
    }

    #[test]
    fn passwords_hash_and_verify() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong horse battery", &h));
        assert!(check_new_password("short").is_err());
    }

    #[tokio::test]
    async fn invite_session_and_second_factor_flow() {
        let path = std::env::temp_dir().join(format!("diagnostic-auth-{}.db", crate::util::new_key()));
        let pool = crate::db::connect(path.to_str().unwrap()).await.unwrap();
        let p = crate::db::default_project(&pool).await.unwrap().id;

        let owner = create_user(&pool, p, "Owner@Example.com", "Olu", "a long enough password", Role::Owner).await.unwrap();
        assert_eq!(owner.email, "owner@example.com");
        assert!(authenticate(&pool, p, "owner@example.com", "a long enough password").await.unwrap().is_some());
        assert!(authenticate(&pool, p, "owner@example.com", "wrong password!!").await.unwrap().is_none());
        assert!(authenticate(&pool, p, "nobody@example.com", "a long enough password").await.unwrap().is_none());

        let (_, link) = create_invite(&pool, p, "ana@example.com", Role::Member, owner.id).await.unwrap();
        let ana = accept_invite(&pool, p, &link, "Ana", "another long password").await.unwrap();
        assert_eq!(ana.role, Role::Member);
        assert!(accept_invite(&pool, p, &link, "Ana", "another long password").await.is_err(), "links are single use");

        let token = create_session(&pool, ana.id, false, None).await.unwrap();
        let s = session(&pool, p, &token).await.unwrap().unwrap();
        assert!(!s.mfa_ok);

        let enrol = begin_totp(&pool, &ana, "Diagnostic").await.unwrap();
        assert!(enrol.qr_svg.starts_with("<?xml") || enrol.qr_svg.contains("<svg"));
        let code = format!("{:06}", totp_at(&enrol.secret, now_ms() / 1000 / 30).unwrap());
        let recovery = confirm_totp(&pool, ana.id, &code).await.unwrap();
        assert_eq!(recovery.len(), 10);
        assert_eq!(verify_second_factor(&pool, ana.id, &code).await.unwrap(), None, "a code can't be replayed");
        assert_eq!(verify_second_factor(&pool, ana.id, &recovery[0].to_uppercase()).await.unwrap(), Some(SecondFactor::RecoveryCode));
        assert_eq!(verify_second_factor(&pool, ana.id, &recovery[0]).await.unwrap(), None, "recovery codes are single use");
        assert_eq!(recovery_codes_left(&pool, ana.id).await.unwrap(), 9);

        let (_, owner_token) = create_api_token(&pool, p, &owner, "CI", false).await.unwrap();
        assert_eq!(verify_api_token(&pool, p, &owner_token).await.unwrap().unwrap().role(), Role::Owner);
        let (_, ana_token) = create_api_token(&pool, p, &ana, "Claude Code", false).await.unwrap();
        let (_, ana_read) = create_api_token(&pool, p, &ana, "read only", true).await.unwrap();
        assert_eq!(verify_api_token(&pool, p, &ana_token).await.unwrap().unwrap().role(), Role::Member);
        assert_eq!(verify_api_token(&pool, p, &ana_read).await.unwrap().unwrap().role(), Role::Viewer);
        set_role(&pool, p, ana.id, Role::Viewer).await.unwrap();
        assert_eq!(verify_api_token(&pool, p, &ana_token).await.unwrap().unwrap().role(), Role::Viewer, "tokens follow the owner's role");
        assert_eq!(list_api_tokens(&pool, p, Some(ana.id)).await.unwrap().len(), 2);
        assert_eq!(list_api_tokens(&pool, p, None).await.unwrap().len(), 3);
        let first = list_api_tokens(&pool, p, Some(owner.id)).await.unwrap()[0].id;
        assert!(!revoke_api_token(&pool, p, first, Some(ana.id)).await.unwrap(), "people can only revoke their own");
        remove_user(&pool, p, ana.id).await.unwrap();
        assert!(verify_api_token(&pool, p, &ana_token).await.unwrap().is_none(), "removing someone revokes their tokens");
        assert!(remove_user(&pool, p, owner.id).await.is_err(), "the owner stays");
        let _ = std::fs::remove_file(path);
    }
}
