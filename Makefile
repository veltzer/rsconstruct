.PHONY: all
all:
	@cargo build
	@cargo build --release

.PHONY: test
test:
	cargo nextest run --release
	cargo nextest run
# cargo test --release
# cargo test

.PHONY: clean
clean:
	@rm -rf release

.PHONY: rsb_clean_build
rsb_clean_build:
	@target/release/rsbuild clean -v
	@target/release/rsbuild build -j 4 -v

.PHONY: rsb_clean_build_hard
rsb_clean_build_hard:
	@target/release/rsbuild clean -v
	@target/release/rsbuild cache clear -v
	@time target/release/rsbuild build -v

.PHONY: rsb_graph
rsb_graph:
	@target/release/rsbuild graph --view mermaid

.PHONY: rsb_build
rsb_build:
	@target/release/rsbuild build -v

.PHONY: rsb_clean
rsb_clean:
	@target/release/rsbuild clean -v

.PHONY: artifacts
artifacts:
	@gh release view --json assets --jq '.assets[] | "\(.name)\t\(.size)\t\(.downloadCount)"' | column -t -N NAME,SIZE,DOWNLOADS
