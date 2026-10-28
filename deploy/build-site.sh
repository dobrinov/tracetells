#!/bin/sh
# Builds tracetells.com into OUT_DIR: the marketing site from site/, plus the
# Stride demo shop at /demo from the same source `trace serve --demo-site`
# uses. GitHub Pages runs it (.github/workflows/pages.yml); run it locally to
# check the result:
#
#   TRACE_URL=... SHOP_WRITE_KEY=... SHOP_PUBLIC_URL=... deploy/build-site.sh _site
set -eu

out=${1:?usage: deploy/build-site.sh OUT_DIR}
: "${TRACE_URL:?the Trace server, e.g. https://app.tracetells.com}"
: "${SHOP_WRITE_KEY:?the demo shop website's write key}"
: "${SHOP_PUBLIC_URL:?the demo shop's public view link}"

root=$(cd "$(dirname "$0")/.." && pwd)
shop="$root/crates/trace-server/assets/shop"

rm -rf "$out"
mkdir -p "$out/demo/api/reviews"
cp -R "$root/site/." "$out/"

# The shop under /demo: static reviews (one in three requests asks for a file
# that doesn't exist, a real failed request), and the consent bar like the rest
# of tracetells.com.
consent=' data-consent="banner" data-consent-link="https://tracetells.com/privacy.html"'
sed -e "s#{{TRACE_URL}}#${TRACE_URL%/}#g" \
    -e "s#{{WRITE_KEY}}#$SHOP_WRITE_KEY#g" \
    -e "s#{{BASE}}#/demo#g" \
    -e "s#{{REVIEWS}}#static#g" \
    -e "s#{{CONSENT}}#$consent#g" \
    -e "s#{{RECORDED_URL}}#$SHOP_PUBLIC_URL\#/behaviour#g" \
    "$shop/index.html" > "$out/demo/index.html"
cp "$shop/shop.js" "$out/demo/shop.js"
for id in 101 102 103 104 105; do
  printf '{ "rating": 4.6, "count": 128 }\n' > "$out/demo/api/reviews/$id.json"
done
if grep -q '{{' "$out/demo/index.html"; then
  echo "a placeholder is left in demo/index.html" >&2
  exit 1
fi

# GitHub Pages answers every unknown path with this file. Shop paths go back to
# the shop, which picks the path up before its collector starts; anything else
# is a plain "not found".
cat > "$out/404.html" <<'HTML'
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Not found · Trace</title>
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="/style.css">
<script>
  if (location.pathname.indexOf("/demo/") === 0) {
    try { sessionStorage.setItem("stride_path", location.pathname + location.search); } catch (e) {}
    location.replace("/demo/");
  }
</script>
</head>
<body>
<main id="main" class="wrap legal">
  <h1 class="display">Not found</h1>
  <p>There's no page at this address. <a href="/">Go to the home page</a>.</p>
</main>
</body>
</html>
HTML
echo "built $out"
