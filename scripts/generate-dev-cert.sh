#!/bin/bash
# Generate a self-signed code signing certificate for TurboTalk development.
# This provides a stable signing identity so macOS TCC (Accessibility, Input
# Monitoring, Microphone) permissions persist across rebuilds.
#
# With ad-hoc signing (signingIdentity: "-"), every rebuild changes the binary
# hash and macOS treats the app as a new, unknown identity — dropping all
# granted permissions. A self-signed cert has a stable identity.

set -euo pipefail

CERT_NAME="TurboTalk Development"
CERT_DIR="${HOME}/.config/turbotalk/dev-cert"
KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"

mkdir -p "$CERT_DIR"

# Generate key and cert
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$CERT_DIR/dev.key" 2>/dev/null

cat > "$CERT_DIR/cert.conf" << 'EOF'
[req]
distinguished_name = dn
x509_extensions = v3_ext
prompt = no

[dn]
CN = TurboTalk Development
O = TurboTalk
OU = Development

[v3_ext]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = codeSigning
subjectKeyIdentifier = hash
EOF

openssl req -x509 -new -key "$CERT_DIR/dev.key" \
  -out "$CERT_DIR/dev.cer" -days 3650 \
  -config "$CERT_DIR/cert.conf" 2>/dev/null

# Export as PKCS12 with macOS-compatible encryption
openssl pkcs12 -export \
  -inkey "$CERT_DIR/dev.key" \
  -in "$CERT_DIR/dev.cer" \
  -out "$CERT_DIR/dev.p12" \
  -passout pass:turbotalk \
  -keypbe PBE-SHA1-3DES \
  -certpbe PBE-SHA1-3DES \
  -macalg SHA1 \
  -name "$CERT_NAME" 2>/dev/null

# Remove old cert from keychain if it exists
security delete-certificate -c "$CERT_NAME" "$KEYCHAIN" 2>/dev/null || true

# Import into login keychain
security import "$CERT_DIR/dev.p12" -P turbotalk \
  -k "$KEYCHAIN" -A 2>/dev/null

echo "Certificate '$CERT_NAME' created and installed."
echo "Identity is stable across rebuilds — TCC permissions will persist."
echo ""
echo "To verify: codesign -s '$CERT_NAME' --force /tmp/test-sign"