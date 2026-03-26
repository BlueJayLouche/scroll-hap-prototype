#!/bin/bash
# Build script for HAP WASM module

set -e

echo "🏗️  Building HAP WASM module..."

# Check if we're in the right directory
if [ ! -f "hap-wasm/Cargo.toml" ]; then
    echo "❌ Error: Must run from hap-wasm-approach directory"
    exit 1
fi

# Install wasm-pack if not present
if ! command -v wasm-pack &> /dev/null; then
    echo "📦 Installing wasm-pack..."
    curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
fi

cd hap-wasm

# Build for web target
echo "🔨 Compiling Rust to WASM (this may take a minute)..."
wasm-pack build --target web --out-dir pkg

echo ""
echo "✅ Build complete!"
echo ""
echo "📁 Files generated:"
ls -lh pkg/*.js pkg/*.wasm 2>/dev/null || ls -la pkg/

echo ""
echo "🚀 To serve the demo:"
echo "   cd scroll-hap-prototype/hap-wasm-approach"
echo "   python3 -m http.server 8080"
echo "   # or use: npx serve ."
echo ""
echo "🌐 Then open http://localhost:8080"
