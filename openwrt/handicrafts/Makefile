# OpenWrt package for the Rachel's Handicrafts shop, with the LuCI page that
# sets it up (Services > Handicrafts). One package, like Monokulo's.
#
# The binary is cross-compiled with cargo (scripts/build-openwrt.sh) and
# dropped into files/ before the SDK runs: building Rust inside the SDK would
# mean bootstrapping a whole Rust toolchain.
#
# HANDICRAFTS_VERSION, HANDICRAFTS_RELEASE and HANDICRAFTS_COMMIT come from
# the build script (the crate version, the commit count so every build
# upgrades the last, and the commit it was built from).

include $(TOPDIR)/rules.mk

PKG_NAME:=handicrafts
PKG_VERSION:=$(or $(HANDICRAFTS_VERSION),0.1.0)
PKG_RELEASE:=$(or $(HANDICRAFTS_RELEASE),1)
PKG_COMMIT:=$(HANDICRAFTS_COMMIT)
PKG_LICENSE:=GPL-3.0-only

include $(INCLUDE_DIR)/package.mk

define Package/handicrafts
  SECTION:=net
  CATEGORY:=Network
  SUBMENU:=Web Servers/Proxies
  TITLE:=Rachel's Handicrafts: the shop, its orders and payments
  # A static binary. ca-bundle: Tern's Tuta SDK checks Tuta's certificate
  # against the system's CA store (SMTP and Monokulo carry their own roots).
  DEPENDS:=@(aarch64||x86_64) +ca-bundle
  USERID:=handicrafts:handicrafts
endef

define Package/handicrafts/description
  The Rachel's Handicrafts web shop in one static binary: the pages, the
  checkout that saves orders, the /admin orders page, the pay page with
  Monero (through Monokulo) and PayID / bank transfer, and email over SMTP
  or from a Tuta account. Includes its LuCI page under Services > Handicrafts.
endef

define Package/handicrafts/conffiles
/etc/config/handicrafts
endef

define Build/Compile
endef

define Package/handicrafts/install
	$(INSTALL_DIR) $(1)/usr/bin $(1)/etc/init.d $(1)/etc/config $(1)/lib/upgrade/keep.d
	$(INSTALL_BIN) $(CURDIR)/files/handicrafts $(1)/usr/bin/handicrafts
	$(INSTALL_BIN) $(CURDIR)/files/handicrafts.init $(1)/etc/init.d/handicrafts
	$(INSTALL_CONF) $(CURDIR)/files/handicrafts.config $(1)/etc/config/handicrafts
	$(INSTALL_DATA) $(CURDIR)/files/handicrafts.keep $(1)/lib/upgrade/keep.d/handicrafts
	$(INSTALL_DIR) $(1)/usr/share/handicrafts
	printf 'version=%s\ncommit=%s\n' '$(PKG_VERSION)-r$(PKG_RELEASE)' '$(PKG_COMMIT)' \
		>$(1)/usr/share/handicrafts/version
	chmod 0644 $(1)/usr/share/handicrafts/version
	$(INSTALL_DIR) $(1)/usr/share/rpcd/ucode
	$(INSTALL_DATA) $(CURDIR)/files/rpcd.uc $(1)/usr/share/rpcd/ucode/handicrafts
	$(INSTALL_DIR) $(1)/www/luci-static/resources/view
	$(INSTALL_DATA) $(CURDIR)/files/luci/handicrafts.js $(1)/www/luci-static/resources/view/handicrafts.js
	$(INSTALL_DIR) $(1)/usr/share/luci/menu.d $(1)/usr/share/rpcd/acl.d
	$(INSTALL_DATA) $(CURDIR)/files/luci/menu.json $(1)/usr/share/luci/menu.d/luci-app-handicrafts.json
	$(INSTALL_DATA) $(CURDIR)/files/luci/acl.json $(1)/usr/share/rpcd/acl.d/luci-app-handicrafts.json
endef

# LuCI reads its menu and ACLs at login; drop its caches so Services >
# Handicrafts shows up (or goes) without a reboot. On an upgrade, restart so
# the new binary runs (procd keeps an instance whose command line is the same).
define Package/handicrafts/postinst
#!/bin/sh
[ -n "$${IPKG_INSTROOT}" ] && exit 0
mkdir -p /etc/handicrafts/secrets && chmod 700 /etc/handicrafts /etc/handicrafts/secrets
rm -f /tmp/luci-indexcache* /tmp/luci-modulecache/* 2>/dev/null
/etc/init.d/rpcd reload 2>/dev/null
if [ "$${PKG_UPGRADE}" = 1 ] && /etc/init.d/handicrafts running; then
	/etc/init.d/handicrafts restart
fi
exit 0
endef

define Package/handicrafts/postrm
#!/bin/sh
[ -n "$${IPKG_INSTROOT}" ] && exit 0
rm -f /tmp/luci-indexcache* /tmp/luci-modulecache/* 2>/dev/null
/etc/init.d/rpcd reload 2>/dev/null
exit 0
endef

$(eval $(call BuildPackage,handicrafts))
