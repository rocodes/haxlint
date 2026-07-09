.PHONY: debug release deps release-deps

deps:
	@command -v cargo > /dev/null || (echo "cargo is not installed" && exit 1;)

release-deps:
	@command -v npm > /dev/null || (echo "npm is not installed" && exit 1;)
	@command -v vsce > /dev/null || (echo "vsce is not installed" && exit 1;)

debug: deps
	@cargo build
	@cp target/debug/haxlint .
	@echo "Debug build complete. Use Run>Start Debugging in VSCode to test."

release: deps release-deps
	@cargo build --release
	@cp target/release/haxlint .
	@vsce package
	@echo "VSIX packaging complete. Install via VSCode Preferences>Extensions."