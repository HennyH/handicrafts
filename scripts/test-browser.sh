#!/bin/sh
# The shop's whole flow in a real browser (tests/browser/flow.mjs): builds
# the shop, starts it on a fresh database with Monokulo pointed at the fake
# one the test serves, runs the test, and stops the shop.
#
# Needs: cargo, Node 20+, and Chromium or Chrome (CHROME=/path/to/it if it
# isn't /usr/bin/chromium). Screenshots go to target/browser-test/.
set -eu
cd "$(dirname "$0")/.."

cargo build --quiet --bin handicrafts
out=target/browser-test
rm -rf "$out"
mkdir -p "$out/shots"
[ -d tests/browser/node_modules ] || (cd tests/browser && npm ci --silent)

HANDICRAFTS_ADMIN_PASSWORD_HASH=$(echo "correct horse battery" | ./target/debug/handicrafts hash-password)
export HANDICRAFTS_ADMIN_PASSWORD_HASH
export HANDICRAFTS_LISTEN=127.0.0.1:8082
export HANDICRAFTS_PUBLIC_URL=http://localhost:8082
export HANDICRAFTS_DATA_DIR="$out/data"
export HANDICRAFTS_ADMIN_USER=rachel
export HANDICRAFTS_ADMIN_EMAIL=hello@rachelshandicrafts.com
export HANDICRAFTS_MAIL=log
export HANDICRAFTS_PAYID=hello@rachelshandicrafts.com
export HANDICRAFTS_BANK_ACCOUNT_NAME="R Example"
export HANDICRAFTS_BSB=000-000
export HANDICRAFTS_ACCOUNT_NUMBER=12345678
export HANDICRAFTS_MONOKULO_URL=http://127.0.0.1:9099

./target/debug/handicrafts >"$out/shop.log" 2>&1 &
shop=$!
trap 'kill $shop 2>/dev/null || true' EXIT
i=0
until curl -sf -o /dev/null http://127.0.0.1:8082/; do
	i=$((i + 1))
	[ $i -lt 50 ] || { cat "$out/shop.log"; exit 1; }
	sleep 0.2
done

node tests/browser/flow.mjs "$out/shots"
