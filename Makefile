SHELL = /bin/sh
CARGO ?= cargo
JOBS ?= 4
TEST_THREADS ?= 4
INSTALL_DIR ?= /data/scripts
BUILD_PATH = $(HOME)/.cargo/bin:$(PATH):/opt/homebrew/bin:/usr/local/bin

all: test build

build: release

release:
	PATH="$(BUILD_PATH)" $(CARGO) build --locked --release -j $(JOBS)

debug:
	PATH="$(BUILD_PATH)" $(CARGO) build --locked -j $(JOBS)

# The release is as static as this program can be: it loads X11, OpenGL and SDL2 when it
# runs, which a fully static executable cannot do. This checks that it needs nothing else
# than the C library of the system.
static: release
	@set -e; \
	binary=target/release/complex_surface_viewer; \
	case "$$(uname -s)" in \
		Darwin) \
			libraries="$$(otool -L "$$binary" | awk 'NR > 1 { print $$1 }')"; \
			system='^/(usr/lib|System/Library)/' ;; \
		*) \
			libraries="$$(ldd "$$binary" | awk '$$1 !~ /:$$/ { print $$1 }')"; \
			system='^(/|\[vdso\]$$|linux-vdso\.|lib(c|m|gcc_s|thr|sys|pthread|dl|rt|execinfo)\.)' ;; \
	esac; \
	[ -n "$$libraries" ] || { echo "Cannot list the libraries of $$binary" >&2; exit 1; }; \
	others="$$(printf '%s\n' "$$libraries" | grep -Ev "$$system" | tr '\n' ' ')"; \
	[ -z "$$others" ] || { echo "$$binary needs more than the C library of the system: $$others" >&2; exit 1; }; \
	printf 'Static-profile executable: %s\nOnly the C library of the system is dynamic; X11, OpenGL and SDL2 are loaded when the program runs.\n' "$$binary"

install: release static
	install -m 755 target/release/complex_surface_viewer complex_surface_viewer
	install -m 755 target/release/complex_surface_viewer complex_surface_viewer.static
	mkdir -p "$(INSTALL_DIR)"
	install -m 755 complex_surface_viewer.static "$(INSTALL_DIR)/complex_surface_viewer"

test:
	PATH="$(BUILD_PATH)" RUST_TEST_THREADS=$(TEST_THREADS) $(CARGO) test --locked -j $(JOBS)

lint:
	PATH="$(BUILD_PATH)" $(CARGO) clippy --locked --all-targets -j $(JOBS)

clean:
	PATH="$(BUILD_PATH)" $(CARGO) clean

.PHONY: all build release debug static install test lint clean
