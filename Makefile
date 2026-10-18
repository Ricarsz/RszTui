PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO ?= cargo

.PHONY: all build install uninstall test check fmt clean

all: build

build:
	$(CARGO) build --release

install: build
	install -Dm755 target/release/rogtui $(DESTDIR)$(BINDIR)/rogtui

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/rogtui

test:
	$(CARGO) test

check:
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean
