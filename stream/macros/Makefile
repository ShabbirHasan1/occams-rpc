RUNTESTCASE = _run_test_case() {                                                  \
    case="$(filter-out $@,$(MAKECMDGOALS))";                                      \
    if [ -n "$${case}" ]; then                                                    \
        RUST_BACKTRACE=full cargo test $${case} -- --nocapture --test-threads=1;  \
    else                                                                          \
        RUST_BACKTRACE=full cargo test -- --nocapture --test-threads=1;           \
    fi  \
}

.PHONY: init
init: git-hooks

.PHONY: test
test: init
	@echo "Run test"
	@${RUNTESTCASE}; _run_test_case
	@echo "Done"

.PHONY: build
build: init
	cargo build

.DEFAULT_GOAL = build

# Target name % means that it is a rule that matches anything, @: is a recipe;
# the : means do nothing
%:
	@:

