#!/bin/sh
# Build the OpenWrt package (.apk) for one OpenWrt release and CPU
# architecture (by default 25.12 on the GL.iNet Flint 2: mediatek/filogic,
# aarch64_cortex-a53) and a signed apk repository holding it. Adapted from
# Monokulo's scripts/build-openwrt.sh.
#
# The binary is cross-compiled here with cargo. Its C parts (SQLite, ring)
# are compiled with the gcc from the official OpenWrt SDK image, so they are
# built exactly as OpenWrt builds C. The same SDK (in Docker) then packages
# the binary with its init script and LuCI page, and signs the package and
# the repository index.
#
# Needs: docker, rustup, openssl.
#
# Output:
#   dist/          the package, the repository index and the public key
#   dist/repo/     the signed apk repository: the package and packages.adb
#   dist/throwaway.pem  the public half of the throwaway key, when one signed it
#
# Signing key (must match openwrt/keys/monokulo.pem, the key that also signs
# Monokulo and Kringle), first found wins:
#   $APK_SIGNING_KEY       the PEM itself (how CI passes the secret)
#   $HANDICRAFTS_SIGNING_KEY  path to the PEM
#   ~/.config/kringle/apk-signing-key.pem
# Without one the package is signed with a throwaway key, made here: fine for
# testing, but routers that trust monokulo.pem will refuse it.
set -eu
cd "$(dirname "$0")/.."

SDK_IMAGE="${SDK_IMAGE:-openwrt/sdk:mediatek-filogic-25.12.5}"
ARCH="${OPENWRT_ARCH:-aarch64_cortex-a53}"
TARGET="${RUST_TARGET:-aarch64-unknown-linux-musl}"
TOOLCHAIN_PREFIX="${TOOLCHAIN_PREFIX:-aarch64-openwrt-linux-musl}"
triple=$(printf '%s' "$TARGET" | tr - _)
TRIPLE=$(printf '%s' "$triple" | tr '[:lower:]' '[:upper:]')
PUBKEY=openwrt/keys/monokulo.pem
PKG=handicrafts

VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
PKG_RELEASE=$(git rev-list --count HEAD 2>/dev/null || echo 1)
COMMIT=$(git rev-parse HEAD 2>/dev/null || true)

key="${APK_SIGNING_KEY:-}"
if [ -z "$key" ]; then
	keyfile="${HANDICRAFTS_SIGNING_KEY:-$HOME/.config/kringle/apk-signing-key.pem}"
	[ -f "$keyfile" ] && key=$(cat "$keyfile")
fi
if [ -n "$key" ]; then
	if ! printf '%s\n' "$key" | openssl ec -pubout 2>/dev/null | cmp -s - "$PUBKEY"; then
		echo "error: the signing key doesn't match $PUBKEY" >&2
		exit 1
	fi
else
	echo "warning: no signing key, so the package is signed with a throwaway key" >&2
	key=$(openssl ecparam -name prime256v1 -genkey -noout 2>/dev/null)
	throwaway=1
fi

# The SDK's cross toolchain, copied out of its image once per image tag.
tag=${SDK_IMAGE##*:}
toolchain_root="${XDG_CACHE_HOME:-$HOME/.cache}/handicrafts/openwrt-sdk/$tag"
if [ ! -d "$toolchain_root/staging_dir" ]; then
	rm -rf "$toolchain_root"
	mkdir -p "$toolchain_root"
	container=$(docker create "$SDK_IMAGE")
	trap 'docker rm -f "$container" >/dev/null 2>&1 || true' EXIT
	docker cp "$container:/builder/staging_dir" "$toolchain_root/staging_dir.partial" >/dev/null
	docker rm -f "$container" >/dev/null
	trap - EXIT
	mv "$toolchain_root/staging_dir.partial" "$toolchain_root/staging_dir"
fi
toolchain=$(ls -d "$toolchain_root"/staging_dir/toolchain-"$ARCH"_gcc-*_musl* | head -n 1)
[ -x "$toolchain/bin/$TOOLCHAIN_PREFIX-gcc" ] || {
	echo "error: no $TOOLCHAIN_PREFIX-gcc in $SDK_IMAGE" >&2
	exit 1
}

rustup target add "$TARGET"
(
	export STAGING_DIR="$toolchain_root/staging_dir"
	export PATH="$toolchain/bin:$PATH"
	export "CC_$triple=$TOOLCHAIN_PREFIX-gcc"
	export "AR_$triple=$TOOLCHAIN_PREFIX-ar"
	# Tuta's post-quantum crypto (pqclean, in pqcrypto-mlkem) checks the gcc
	# version with glibc's __GNUC_PREREQ, which musl's headers don't have.
	export "CFLAGS_$triple=-D__GNUC_PREREQ(maj,min)=((__GNUC__<<16)+__GNUC_MINOR__>=((maj)<<16)+(min))"
	export "CARGO_TARGET_${TRIPLE}_LINKER=$TOOLCHAIN_PREFIX-gcc"
	export CARGO_PROFILE_RELEASE_STRIP=symbols
	cargo build --release --locked --target "$TARGET" --bin "$PKG"
)
bin="target/$TARGET/release/$PKG"
# x86_64's musl target links a static PIE, aarch64's a plain static
# executable: either runs on OpenWrt, a dynamic one doesn't.
if ! file "$bin" | grep -Eq 'statically linked|static-pie linked'; then
	echo "error: $PKG isn't a static executable, so it won't run on OpenWrt:" >&2
	file "$bin" >&2
	exit 1
fi
install -m 0755 "$bin" "openwrt/$PKG/files/$PKG"

rm -rf dist
mkdir -p dist/repo
chmod 0777 dist dist/repo # the SDK runs as its own user

SDK_VOLUME="${SDK_VOLUME-handicrafts-sdk-$tag}"
set -- -v "$PWD/openwrt:/feed:ro" -v "$PWD/dist:/dist"
[ -n "$SDK_VOLUME" ] && set -- "$@" -v "$SDK_VOLUME:/builder"

# The key reaches the container through the environment, never on docker's
# command line where other local users could see it.
APK_KEY="$key" docker run --rm -e APK_KEY -e PKG="$PKG" \
	-e HANDICRAFTS_VERSION="$VERSION" -e HANDICRAFTS_RELEASE="$PKG_RELEASE" -e HANDICRAFTS_COMMIT="$COMMIT" \
	"$@" "$SDK_IMAGE" sh -euc '
	[ -x ./setup.sh ] && [ ! -f rules.mk ] && ./setup.sh

	trap "rm -f private-key.pem" EXIT
	rm -f private-key.pem public-key.pem
	(umask 077 && printf "%s\n" "$APK_KEY" > private-key.pem)
	openssl ec -in private-key.pem -pubout -out public-key.pem 2>/dev/null

	grep -q "^src-link handicrafts " feeds.conf.default || echo "src-link handicrafts /feed" >> feeds.conf.default
	./scripts/feeds update handicrafts
	./scripts/feeds install $PKG
	make defconfig
	rm -rf bin/packages/*/handicrafts build_dir/target-*/handicrafts-*
	make "package/$PKG/compile" -j"$(nproc)" \
		HANDICRAFTS_VERSION="$HANDICRAFTS_VERSION" HANDICRAFTS_RELEASE="$HANDICRAFTS_RELEASE" \
		HANDICRAFTS_COMMIT="$HANDICRAFTS_COMMIT"
	# Sign the package too, so a downloaded .apk installs directly (apk add,
	# or LuCI Upload Package) on any router that trusts monokulo.pem.
	mkdir -p /tmp/handicrafts-keys
	cp public-key.pem /tmp/handicrafts-keys/monokulo.pem
	for f in bin/packages/*/handicrafts/*.apk; do
		staging_dir/host/bin/apk --allow-untrusted adbsign --sign-key private-key.pem "$f"
		staging_dir/host/bin/apk --keys-dir /tmp/handicrafts-keys verify "$f"
	done
	make package/index
	cp bin/packages/*/handicrafts/* /dist/repo/
'

cp dist/repo/*.apk dist/repo/packages.adb dist/
cp "$PUBKEY" dist/monokulo.pem
if [ -n "${throwaway:-}" ]; then
	printf '%s\n' "$key" | openssl ec -pubout 2>/dev/null >dist/throwaway.pem
fi
ls dist/"$PKG"-[0-9]*.apk >/dev/null || {
	echo "error: the SDK built no $PKG package" >&2
	exit 1
}
(cd dist && sha256sum ./*.apk packages.adb monokulo.pem > SHA256SUMS)
