PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO ?= cargo

.PHONY: all build install uninstall test check fmt pkg setup-nopasswd remove-nopasswd clean

all: build

build:
	$(CARGO) build --release

install: build
	install -Dm755 target/release/rsztui $(DESTDIR)$(BINDIR)/rsztui

pkg:
	cd packaging/aur && makepkg -f --noconfirm
	@ls -lh packaging/aur/*.pkg.tar.zst

setup-nopasswd:
	./scripts/setup-nopasswd.sh

remove-nopasswd:
	./scripts/remove-nopasswd.sh

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/rsztui

test:
	$(CARGO) test

check:
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean
