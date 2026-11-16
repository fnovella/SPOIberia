#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT_DIR="$(pwd)"
mkdir -p dist

export PATH="$HOME/.local/share/mise/shims:$HOME/.cargo/bin:$PATH"

echo "==> [SPOIberia] Preparando entorno de compilación multiplataforma..."

if ! command -v zig >/dev/null 2>&1; then
    if command -v mise >/dev/null 2>&1; then
        echo "--> Instalando zig en espacio de usuario mediante mise..."
        mise use -g zig@0.13.0
    fi
fi

if ! command -v cargo-zigbuild >/dev/null 2>&1; then
    echo "--> Instalando cargo-zigbuild..."
    cargo install --locked cargo-zigbuild
fi

TARGETS=(
    "x86_64-unknown-linux-gnu:spoiberia:spoiberia-linux-x86_64"
    "x86_64-pc-windows-gnu:spoiberia.exe:spoiberia-windows-x86_64.exe"
    "aarch64-apple-darwin:spoiberia:spoiberia-macos-aarch64"
)

for entry in "${TARGETS[@]}"; do
    IFS=":" read -r target src_bin out_bin <<< "$entry"
    echo "==> Añadiendo target Rust: $target"
    rustup target add "$target"

    if [[ "$target" == "x86_64-unknown-linux-gnu" ]]; then
        echo "==> Compilando nativamente para $target..."
        cargo build --release --target "$target"
    elif [[ "$target" == *"-apple-darwin" ]]; then
        echo "==> Compilando cruzado con cargo-zigbuild para $target..."
        RUSTFLAGS="-L framework=${ROOT_DIR}/scripts/macos-frameworks" \
            cargo zigbuild --release --target "$target"
    else
        echo "==> Compilando cruzado con cargo-zigbuild para $target..."
        cargo zigbuild --release --target "$target"
    fi

    cp "target/$target/release/$src_bin" "dist/${out_bin}.tmp"
    chmod +x "dist/${out_bin}.tmp"
    mv -f "dist/${out_bin}.tmp" "dist/$out_bin"
    echo "    ✓ Generado: dist/$out_bin"
done

(
    cd dist
    sha256sum spoiberia-* > SHA256SUMS
)

echo ""
echo "==> ¡Binarios multiplataforma generados en ./dist/!"
ls -lh dist/
