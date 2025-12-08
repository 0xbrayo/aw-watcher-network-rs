.PHONY: all build install clean

# Determine PREFIX based on whether we're using sudo or not
DESTDIR :=
ifeq ($(SUDO_USER),)
    PREFIX := $(HOME)/.local
else
    PREFIX := /usr/local
endif

# Build in release mode by default, unless RELEASE=false
ifeq ($(RELEASE), false)
	CARGO_FLAGS :=
	TARGET_DIR := debug
else
	CARGO_FLAGS := --release
	TARGET_DIR := release
endif

all: build

build:
	cargo build $(CARGO_FLAGS)

install: build
	# Install aw-watcher-network-rs executable
	mkdir -p $(DESTDIR)$(PREFIX)/bin/
	install -m 755 target/$(TARGET_DIR)/aw-watcher-network-rs $(DESTDIR)$(PREFIX)/bin/aw-watcher-network-rs
	# Install systemd user service
ifeq ($(SUDO_USER),)
	mkdir -p $(HOME)/.config/systemd/user
	install -m 644 aw-watcher-network.service $(HOME)/.config/systemd/user/aw-watcher-network.service
	systemctl --user daemon-reload || true
else
	mkdir -p $(DESTDIR)$(PREFIX)/lib/systemd/user
	install -m 644 aw-watcher-network.service $(DESTDIR)$(PREFIX)/lib/systemd/user/aw-watcher-network.service
	systemctl daemon-reload || true
endif

clean:
	cargo clean
