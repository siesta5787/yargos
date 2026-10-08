YARGOS_HOME_VERSION = 0.1.0
YARGOS_HOME_SITE = $(BR2_EXTERNAL_YARGOS_PATH)/src/home
YARGOS_HOME_SITE_METHOD = local
YARGOS_HOME_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = --exclude target --exclude VENDOR
YARGOS_HOME_LICENSE = MIT

# Buildroot vendors a Cargo package's crates in its download step, which a local source skips, and then
# builds with --offline. Do the same vendoring here (needs the network the first time; crates are
# cached in the Buildroot download directory).
define YARGOS_HOME_VENDOR_CRATES
	cd $(@D) && mkdir -p .cargo && \
	PATH="$(HOST_DIR)/bin:$$PATH" CARGO_HOME=$(BR_CARGO_HOME) \
		cargo vendor --locked VENDOR > .cargo/config.toml
endef
YARGOS_HOME_PRE_BUILD_HOOKS += YARGOS_HOME_VENDOR_CRATES

$(eval $(cargo-package))
