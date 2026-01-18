.PHONY: all
all:
	@cargo build
	@cargo build --release

.PHONY: test
test:
	@cargo test

.PHONY: clean_build
clean_build:
	@target/release/rsb clean
	@target/release/rsb build -j 4

.PHONY: graph
graph:
	@target/release/rsb graph --view mermaid

.PHONY: clean
clean:
	@rm -rf release
