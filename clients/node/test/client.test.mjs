// Against a stand-in for Trace: what the client sends from Express, Fastify and
// plain http apps, and what it must never send.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import http from "node:http";
import express from "express";
import Fastify from "fastify";
import * as trace from "../dist/esm/index.js";

const vectors = JSON.parse(readFileSync(new URL("../../test-vectors.json", import.meta.url), "utf8"));

test("scrubbing matches the shared vectors", () => {
  for (const [input, want] of vectors.message) assert.equal(trace.scrub.message(input), want, `message ${input}`);
  for (const [input, want] of vectors.route) assert.equal(trace.scrub.route(input), want, `route ${input}`);
  for (const [input, want] of vectors.file) assert.equal(trace.scrub.file(input), want, `file ${input}`);
});

function listen(server) {
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
}

test("reports errors from Express, Fastify and http without request data", async () => {
  const received = [];
  const sink = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      received.push({ auth: req.headers.authorization, body: JSON.parse(body) });
      res.statusCode = 202;
      res.end("{}");
    });
  });
  const sinkPort = await listen(sink);
  trace.init({ key: "tse_test", endpoint: `http://127.0.0.1:${sinkPort}`, environment: "test", release: "v1.2.3" });

  const secrets = { headers: { cookie: "session=SECRETCOOKIE", authorization: "Bearer SECRETBEARER", "content-type": "application/json" }, body: JSON.stringify({ password: "SECRETBODY" }) };

  // Express
  const app = express();
  app.use(express.json());
  app.post("/orders/:id", () => {
    throw new Error("card 4111 1111 1111 1111 for anna@example.com");
  });
  app.use(trace.expressErrorHandler());
  app.use((err, req, res, next) => res.status(500).send("oops"));
  const ex = http.createServer(app);
  const exPort = await listen(ex);
  const r1 = await fetch(`http://127.0.0.1:${exPort}/orders/8817?coupon=SECRETCOUPON`, { method: "POST", ...secrets });
  assert.equal(r1.status, 500);

  // Fastify
  const f = Fastify();
  await f.register(trace.fastifyPlugin);
  f.get("/users/:id/settings", async () => {
    class SettingsError extends Error {}
    throw new SettingsError("token sk_live_51HkT9a2eZvKYlo2C0jw8Ln is invalid");
  });
  await f.listen({ port: 0, host: "127.0.0.1" });
  const r2 = await fetch(`http://127.0.0.1:${f.server.address().port}/users/42/settings?x=SECRETCOUPON`, { headers: secrets.headers });
  assert.equal(r2.status, 500);

  // Plain http
  const plain = http.createServer(trace.wrapHandler(async () => { throw new TypeError("plain failure"); }, () => "/plain"));
  const plainPort = await listen(plain);
  const r3 = await fetch(`http://127.0.0.1:${plainPort}/plain/123`, { headers: secrets.headers });
  assert.equal(r3.status, 500);

  // A storm: the first at once, the rest as one count.
  for (let i = 0; i < 50; i++) trace.report(new RangeError("storm"));

  await trace.flush(3000);
  const errors = received.flatMap((r) => r.body.errors);
  assert.ok(received.every((r) => r.auth === "Bearer tse_test"));

  const exErr = errors.find((e) => e.route === "/orders/:id");
  assert.equal(exErr.method, "POST");
  assert.equal(exErr.status, 500);
  assert.equal(exErr.message, "card [card] for [email]");
  assert.equal(exErr.environment, "test");
  assert.equal(exErr.release, "v1.2.3");
  assert.equal(exErr.runtime.name, "node");
  assert.ok(exErr.backtrace.some((f) => f.in_app && f.file.startsWith("test/")), JSON.stringify(exErr.backtrace));

  const fErr = errors.find((e) => e.class === "SettingsError");
  assert.equal(fErr.route, "/users/:id/settings");
  assert.equal(fErr.message, "token [token] is invalid");

  const plainErr = errors.find((e) => e.class === "TypeError");
  assert.equal(plainErr.route, "/plain");

  const storm = errors.filter((e) => e.class === "RangeError");
  assert.deepEqual(storm.map((e) => e.count), [1, 49]);

  const everything = JSON.stringify(received.map((r) => r.body));
  for (const secret of ["SECRETCOUPON", "SECRETCOOKIE", "SECRETBEARER", "SECRETBODY", "4111", "anna@example.com", "sk_live", "/Users/", "/home/"]) {
    assert.ok(!everything.includes(secret), `${secret} was sent`);
  }
  for (const key of ["headers", "cookies", "params", "body", "user", "context", "query"]) {
    assert.ok(errors.every((e) => !(key in e)), `a ${key} field was sent`);
  }

  await trace.close();
  await f.close();
  for (const s of [sink, ex, plain]) s.close();
});

test("does nothing without a key", async () => {
  delete process.env.TRACE_SERVER_KEY;
  trace.init({});
  trace.report(new Error("nowhere"));
  await trace.flush(100);
});

test("an uncaught exception is sent, then the process exits as it would without the client", async () => {
  const received = [];
  const sink = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      received.push(JSON.parse(body));
      res.statusCode = 202;
      res.end("{}");
    });
  });
  const port = await listen(sink);
  const { spawn } = await import("node:child_process");
  const child = spawn(process.execPath, ["--input-type=module", "-e", `
    import * as trace from ${JSON.stringify(new URL("../dist/esm/index.js", import.meta.url).href)};
    trace.init({ key: "tse_test", endpoint: "http://127.0.0.1:${port}" });
    setTimeout(() => { throw new Error("crash at startup"); }, 10);
  `]);
  let stderr = "";
  child.stderr.on("data", (c) => (stderr += c));
  const code = await new Promise((r) => child.on("exit", r));
  sink.close();
  assert.equal(code, 1, "exits with 1, like Node's default");
  assert.match(stderr, /crash at startup/, "and prints the error");
  assert.equal(received[0].errors[0].message, "crash at startup");
  assert.equal(received[0].errors[0].class, "Error");
});
