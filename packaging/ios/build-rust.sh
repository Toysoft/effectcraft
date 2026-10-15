#!/bin/sh
# Xcode build phase: build a pure-Rust UIKit executable and place it in the app bundle.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
toolchain=${EFFECTCRAFT_RUST_TOOLCHAIN:-nightly}
case "${PLATFORM_NAME:-iphonesimulator}" in
    iphoneos) rust_target=aarch64-apple-ios ;;
    iphonesimulator)
        case "${CURRENT_ARCH:-arm64}" in
            arm64|undefined_arch) rust_target=aarch64-apple-ios-sim ;;
            x86_64) rust_target=x86_64-apple-ios ;;
            *) echo "Unsupported simulator architecture: $CURRENT_ARCH" >&2; exit 1 ;;
        esac ;;
    *) echo "Use the iOS device or simulator SDK." >&2; exit 1 ;;
esac
export IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET:-18.0}
export CARGO_TARGET_DIR="$repo_dir/target/ios"
cd "$repo_dir"
if [ "${CONFIGURATION:-Debug}" = Release ]; then
    cargo "+$toolchain" build --locked -p effectcraft-ios --target "$rust_target" --release
    profile=release
else
    cargo "+$toolchain" build --locked -p effectcraft-ios --target "$rust_target"
    profile=debug
fi
mkdir -p "${TARGET_BUILD_DIR:?}/${EXECUTABLE_FOLDER_PATH:?}"
cp "$CARGO_TARGET_DIR/$rust_target/$profile/effectcraft-ios" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
chmod 755 "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
