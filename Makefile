.PHONY: all
all:
	@cargo build
	@cargo build --release

.PHONY: test
test:
	@cargo test

.PHONY: graph
graph:
	./target/release/rsb graph --view mermaid

.PHONY: clean
clean:
	@rm -rf release
