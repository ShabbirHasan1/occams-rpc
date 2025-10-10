RUNTESTCASE = _run_test_case() {                                                  \
    case="$(filter-out $@,$(MAKECMDGOALS))";                                      \
    if [ -n "$${case}" ]; then                                                    \
        RUST_BACKTRACE=full cargo test $${case} -- --nocapture --test-threads=1;  \
    else                                                                          \
        RUST_BACKTRACE=full cargo test -- --nocapture --test-threads=1;           \
    fi  \
}

RUNBENCHCASE = _run_bench_case() {                                                  \
    case="$(filter-out $@,$(MAKECMDGOALS))";                                      \
    if [ -n "$${case}" ]; then                                                    \
        RUST_BACKTRACE=full cargo test $${case} --release -- --nocapture --test-threads=1;  \
    else                                                                          \
        echo should specify test case;                                            \
    fi  \
}


INSTALL_GITHOOKS = _install_githooks() {                \
	git config core.hooksPath ./git-hooks;              \
}

.PHONY: git-hooks
git-hooks:
	@$(INSTALL_GITHOOKS); _install_githooks

.PHONY: init
init: git-hooks

.PHONY: fmt
fmt: init
	cargo fmt

.PHONY: test
test: test-core test-codec test-stream-macros test-stream
	@echo "Run test"
	@${RUNTESTCASE}; _run_test_case
	@echo "Done"


.PHONY: test-core
test-core: init
	cargo check -p occams-rpc-core
	cargo test -p occams-rpc-core

.PHONY: test-codec
	cargo check -p occams-rpc-codec
	cargo test -p occams-rpc-codec

.PHONY: test-stream-macros
test-stream-macros: init
	cargo test -p occams-rpc-stream-macros

.PHONY: test-stream
test-stream: init
	@echo "Run stream integration tests"
	cargo test -p stream_test $@ -- --nocapture --test-threads=1
	@echo "Done"

.PHONY: bench
bench:
	@${RUNBENCHCASE}; _run_bench_case

.PHONY: build
build: init
	cargo build -p occams-rpc-core
	cargo build -p occams-rpc-tokio
	cargo build -p occams-rpc-smol
	cargo build -p occams-rpc-stream-macros
	cargo build -p occams-rpc-stream
	cargo build -p occams-rpc-tcp
	cargo build -p stream_test

.DEFAULT_GOAL = build

# Target name % means that it is a rule that matches anything, @: is a recipe;
# the : means do nothing
%:
	@:

