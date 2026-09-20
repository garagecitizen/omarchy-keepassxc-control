PLUGIN_ID := keepassxc.control
PLUGIN_DIR := $(HOME)/.config/omarchy/plugins/$(PLUGIN_ID)
HELPER := keepassxc-control-helper
HELPER_SRC := helper/target/release/$(HELPER)

.PHONY: all build test validate install restart reload clean fixture

all: build

build:
	cargo build --release --manifest-path helper/Cargo.toml
	cp -f $(HELPER_SRC) ./$(HELPER).tmp
	mv -f ./$(HELPER).tmp ./$(HELPER)

test:
	cargo test --manifest-path helper/Cargo.toml
	node Model.test.js

validate:
	omarchy plugin validate .

install: build
	mkdir -p $(HOME)/.config/omarchy/plugins
	ln -sfn $(CURDIR) $(PLUGIN_DIR)
	omarchy plugin validate $(CURDIR)
	omarchy-shell shell rescanPlugins
	omarchy plugin enable $(PLUGIN_ID)
	omarchy bar move $(PLUGIN_ID) --section right

restart:
	omarchy restart shell

reload: build restart

fixture:
	cargo run --manifest-path helper/Cargo.toml --features generate-fixture --bin create-synthetic-db -- --force

clean:
	cargo clean --manifest-path helper/Cargo.toml
	rm -f $(HELPER)
