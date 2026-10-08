YARG_VERSION = 0.15.0
YARG_SOURCE = YARG_v$(YARG_VERSION)-Linux-x86_64.zip
YARG_SITE = https://github.com/YARC-Official/YARG/releases/download/v$(YARG_VERSION)
YARG_LICENSE = LGPL-3.0 (YARG itself), proprietary (bundled BASS, Discord SDK)
YARG_REDISTRIBUTE = NO

define YARG_EXTRACT_CMDS
	mkdir -p $(@D)/payload
	$(UNZIP) -q -d $(@D)/payload $(YARG_DL_DIR)/$(YARG_SOURCE)
endef

define YARG_INSTALL_TARGET_CMDS
	mkdir -p $(TARGET_DIR)/usr/share/yarg-seed/$(YARG_VERSION)
	cp -a $(@D)/payload/. $(TARGET_DIR)/usr/share/yarg-seed/$(YARG_VERSION)/
	chmod 0755 $(TARGET_DIR)/usr/share/yarg-seed/$(YARG_VERSION)/YARG
	echo $(YARG_VERSION) > $(TARGET_DIR)/usr/share/yarg-seed/VERSION
endef

$(eval $(generic-package))
