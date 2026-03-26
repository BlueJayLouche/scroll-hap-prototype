#!/bin/bash
# Simple HTTP server for testing

PORT=${1:-8080}

echo "🚀 Starting HTTP server on port $PORT..."
echo "🌐 Open http://localhost:$PORT"
echo ""

# Python 3 with proper MIME types
if command -v python3 &> /dev/null; then
    python3 << EOF
import http.server
import socketserver
import mimetypes

# Add WASM mime type
mimetypes.add_type('application/wasm', '.wasm')
mimetypes.add_type('application/javascript', '.js')

class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        # Add CORS headers for WASM
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        super().end_headers()

with socketserver.TCPServer(("", $PORT), Handler) as httpd:
    print(f"Serving at http://localhost:$PORT")
    httpd.serve_forever()
EOF
elif command -v python &> /dev/null; then
    python -m http.server $PORT
elif command -v npx &> /dev/null; then
    npx serve . -p $PORT --cors
else
    echo "❌ No suitable server found. Please install Python or Node.js"
    exit 1
fi
