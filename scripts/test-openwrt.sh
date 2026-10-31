#!/bin/sh
# Boot OpenWrt (the official x86_64 rootfs image, with its own procd, ubus,
# rpcd and uhttpd) in Docker and test the package scripts/build-openwrt.sh
# built for x86_64, the way a router runs it: installed by name from the
# signed repository, set up through LuCI's backend, started, refusing a bad
# setting, upgraded keeping its settings and secrets, and removed. A cut-down
# version of Monokulo's scripts/test-openwrt.sh.
#
# Usage: scripts/test-openwrt.sh [dist]
# Needs: docker (the container runs --privileged: procd as PID 1 needs it).
set -eu
cd "$(dirname "$0")/.."

DIST=$(cd "${1:-dist}" && pwd)
IMAGE="${ROOTFS_IMAGE:-openwrt/rootfs:x86-64-25.12.5}"
C=handicrafts-openwrt-test-$$
KEY=$DIST/throwaway.pem
[ -f "$KEY" ] || KEY=$PWD/openwrt/keys/monokulo.pem
for f in "$DIST/repo/packages.adb" "$KEY"; do
	[ -f "$f" ] || {
		echo "error: no $f: run scripts/build-openwrt.sh for x86_64 first" >&2
		exit 1
	}
done

passed=0
cleanup() {
	docker rm -f "$C" >/dev/null 2>&1 || true
}
trap cleanup EXIT

x() {
	docker exec -i "$C" sh -c "$1"
}

fail() {
	echo "not ok - $1" >&2
	echo "--- the router's log:" >&2
	x 'logread | tail -n 60' >&2 || true
	exit 1
}

check() {
	x "$2" >/dev/null 2>&1 || {
		echo "--- $2" >&2
		x "$2" >&2 || true
		fail "$1"
	}
	passed=$((passed + 1))
	echo "ok - $1"
}

eventually() {
	i=0
	until x "$2" >/dev/null 2>&1; do
		i=$((i + 1))
		[ $i -lt 60 ] || {
			echo "--- $2" >&2
			x "$2" >&2 || true
			fail "$1"
		}
		sleep 0.5
	done
	passed=$((passed + 1))
	echo "ok - $1"
}

docker run -d --privileged --name "$C" -v "$DIST:/dist:ro" "$IMAGE" /sbin/init >/dev/null
eventually "OpenWrt boots: procd, ubus and rpcd" 'ubus call system board'

x 'cat >/tmp/t.sh' <<'EOF'
running() {
	ubus call service list '{"name":"handicrafts"}' | jsonfilter -q -e "@.handicrafts.instances.*.running" | grep -q true
}
EOF

docker cp "$KEY" "$C:/etc/apk/keys/monokulo.pem"
x ': >/etc/apk/repositories.d/distfeeds.list; echo /dist/repo/packages.adb >/etc/apk/repositories.d/customfeeds.list'
check "the repository's index is signed by the key the router trusts" 'apk update'
check "handicrafts installs by name from the repository" 'apk add handicrafts'
check "its user exists" 'id -u handicrafts'
check "its LuCI page is installed: view, menu and ACL" \
	'[ -s /www/luci-static/resources/view/handicrafts.js ] && [ -s /usr/share/luci/menu.d/luci-app-handicrafts.json ] && [ -s /usr/share/rpcd/acl.d/luci-app-handicrafts.json ]'
eventually "rpcd serves its ubus object" 'ubus -S list handicrafts'
check "the secrets folder is root's alone" '[ "$(ls -ldn /etc/handicrafts/secrets | cut -c1-10)" = drwx------ ]'

check "LuCI's backend sets the admin password, keeping only an argon2 hash" \
	'[ "$(ubus call handicrafts set_password "{\"password\":\"correct horse battery\"}" | jsonfilter -e @.ok)" = true ] && grep -q "^\$argon2" /etc/handicrafts/secrets/HANDICRAFTS_ADMIN_PASSWORD_HASH && [ "$(ls -ln /etc/handicrafts/secrets/HANDICRAFTS_ADMIN_PASSWORD_HASH | cut -c1-10)" = -rw------- ]'
check "LuCI's backend refuses a short password" \
	'[ "$(ubus call handicrafts set_password "{\"password\":\"short\"}" | jsonfilter -e @.ok)" = false ]'
check "LuCI's backend refuses an unknown secret" \
	'[ "$(ubus call handicrafts set_secret "{\"name\":\"PATH\",\"value\":\"x\"}" | jsonfilter -e @.ok)" = false ]'

x 'uci set handicrafts.main.public_url=http://127.0.0.1:8082; uci set handicrafts.mail.transport=log; uci commit handicrafts'
check "the settings check passes" '/etc/init.d/handicrafts check'
check "LuCI's backend starts it" '[ "$(ubus call handicrafts restart | jsonfilter -e @.result)" = true ]'
eventually "it runs, as its own user" \
	'. /tmp/t.sh; running && [ "$(awk "/^Uid:/ { print \$2 }" /proc/$(pidof handicrafts)/status)" = "$(id -u handicrafts)" ]'
eventually "it serves the shop" 'uclient-fetch -q -O - http://127.0.0.1:8082/ | grep -q "Rachel"'
check "/admin asks for the password" '! uclient-fetch -q -O /dev/null http://127.0.0.1:8082/admin 2>/dev/null'
check "LuCI's status names no secret's value" \
	's=$(ubus call handicrafts status); [ "$(echo "$s" | jsonfilter -e @.running)" = true ] && ! echo "$s" | grep -q argon2'
check "the database is in the data folder, owned by the shop" \
	'[ -s /srv/handicrafts/handicrafts.db ] && [ "$(ls -ldn /srv/handicrafts | awk "{print \$3}")" = "$(id -u handicrafts)" ]'

check "with only the Monokulo address set, it waits for the store to be connected at /admin/monokulo" \
	'logread -e handicrafts | grep -q "until the store is connected at /admin/monokulo"'

x 'uci set handicrafts.main.public_url=ftp://nope; uci commit handicrafts'
check "the check refuses a bad setting, saying why" \
	'! /etc/init.d/handicrafts check && grep -q PUBLIC_URL /tmp/handicrafts.check'
x 'uci set handicrafts.main.public_url=http://127.0.0.1:8082; uci commit handicrafts; /etc/init.d/handicrafts check'

x 'uci set handicrafts.payments.payid=hello@example.com; uci commit handicrafts; sha256sum /etc/handicrafts/secrets/HANDICRAFTS_ADMIN_PASSWORD_HASH >/tmp/pw.sum'
check "an upgrade keeps settings and secrets" \
	'apk fix --reinstall handicrafts && [ "$(uci get handicrafts.payments.payid)" = hello@example.com ] && sha256sum -c /tmp/pw.sum'
eventually "it runs after the upgrade" '. /tmp/t.sh; running'
check "the sysupgrade backup keeps settings, secrets and data" \
	'sysupgrade -l | grep -qx /etc/config/handicrafts && sysupgrade -l | grep -q /etc/handicrafts/secrets/ && sysupgrade -l | grep -q /srv/handicrafts/'

check "removing it stops it" 'apk del handicrafts && ! pidof handicrafts'

echo "$passed passed"
