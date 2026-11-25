.PHONY: help fmt clippy test sim bridge web firmware check vectors clean

help:
	@echo "make test      - cargo test workspace"
	@echo "make sim       - run mesh simulator scenarios"
	@echo "make bridge    - run bridge daemon in demo mode"
	@echo "make web       - install + build + test web client"
	@echo "make firmware  - platformio build all reference boards"
	@echo "make check     - full verification (CI equivalent)"
	@echo "make vectors   - regenerate protocol golden vectors"

fmt:
	cargo fmt --all

test:
	cargo test --workspace --all-targets

sim:
	cargo run -p openmesh-sim

bridge:
	cargo run -p openmesh-bridge -- --demo

web:
	pnpm --dir apps/web install
	pnpm --dir apps/web build
	pnpm --dir apps/web test -- --run

firmware:
	cd firmware && pio run

vectors:
	cargo run -p openmesh-core --example gen_vectors

check:
	./scripts/check.sh

clean:
	cargo clean
	rm -rf apps/web/dist
