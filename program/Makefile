# This Makefile is only used to build a serum_dex.so in order for it to be loaded as bpf code during testing
# and fuzzing. As serum_dex uses a deprecated program entrypoint which aligns the bpf bytes differently, it cannot be
# loaded as native code

serum_dex.so:
	git clone https://github.com/project-serum/serum-dex.git third_party/serum_dex
	mkdir -p target/deploy
	cargo build-bpf --manifest-path "third_party/serum_dex/dex/Cargo.toml" --bpf-out-dir target/deploy

.PHONY: clean
clean:
	rm -rf third_party/serum_dex
	rm -f target/deploy/serum_dex.so