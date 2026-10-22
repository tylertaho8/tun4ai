# One entry point for every check: `make check`.
# On macOS it runs everything CI runs. On Linux it runs all of it except what needs macOS
# (pfctl, route/ipconfig, launchd, swiftc): those tests skip themselves, and the tray is
# type-checked for macOS instead of built. Linux needs: rustup target add x86_64-apple-darwin
PYTHON ?= /usr/bin/python3
SING_BOX ?= build/sing-box
export SING_BOX

.PHONY: check shellcheck python install-test tray sing-box

check: shellcheck python install-test tray

$(SING_BOX):
	./scripts/fetch-sing-box.sh $(dir $(SING_BOX))

sing-box: $(SING_BOX)

shellcheck:
	shellcheck -x install.sh uninstall.sh helper/*.sh scripts/*.sh tests/*.sh

python: sing-box
	$(PYTHON) -m unittest discover -s tests -v

install-test: sing-box
	bash tests/test_install.sh

ifeq ($(shell uname -s),Darwin)
tray:
	cd tray && cargo test --locked
else
tray:
	bash tests/tray_portable.sh
	cd tray && TUN4AI_SKIP_PANEL=1 cargo check --locked --all-targets --target x86_64-apple-darwin
endif
