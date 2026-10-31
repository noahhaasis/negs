#!/usr/bin/env bash
# Makes a local certificate authority and a certificate for this machine, so
# an iPad or iPhone on the same network can open the dev server over HTTPS
# (WebGPU and SharedArrayBuffer need a secure context): web/dev-cert.sh, then
# node web/serve.mjs --https. Run it again when the machine's LAN address
# changes; the authority is kept, so the device needs it installed once.
#
# The authority can only vouch for localhost, *.local and private addresses
# (name constraints), and lives outside web/, which serve.mjs serves.
set -euo pipefail

dir="${NEGS_DEV_CERTS:-${XDG_DATA_HOME:-$HOME/.local/share}/negs-dev-certs}"
mkdir -p "$dir"
chmod 700 "$dir"
cd "$dir"

if [[ ! -f ca.key ]]; then
  cat > ca.cnf <<'EOF'
[req]
distinguished_name = dn
prompt = no
x509_extensions = ca
[dn]
CN = Negs dev server (this machine only)
[ca]
basicConstraints = critical, CA:true, pathlen:0
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
nameConstraints = critical, permitted;DNS:localhost, permitted;DNS:local, permitted;IP:127.0.0.0/255.0.0.0, permitted;IP:10.0.0.0/255.0.0.0, permitted;IP:172.16.0.0/255.240.0.0, permitted;IP:192.168.0.0/255.255.0.0
EOF
  (umask 077; openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out ca.key)
  openssl req -new -x509 -key ca.key -config ca.cnf -days 3650 -out ca.crt
fi

# This machine's names and addresses: localhost, <hostname>.local and every
# private IPv4 address it has now.
alt="DNS:localhost,DNS:$(hostname).local,IP:127.0.0.1"
for ip in $(node --input-type=module -e '
  import { networkInterfaces } from "node:os";
  for (const addresses of Object.values(networkInterfaces())) {
    for (const address of addresses ?? []) {
      if (address.family === "IPv4" && !address.internal) console.log(address.address);
    }
  }
'); do
  case "$ip" in 10.*|192.168.*|172.1[6-9].*|172.2[0-9].*|172.3[01].*) alt+=",IP:$ip" ;; esac
done

(umask 077; openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out server.key)
openssl req -new -key server.key -subj "/CN=$(hostname).local" -out server.csr
# Apple accepts at most 825 days from a user-installed authority; stay under a year.
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 397 -out server.crt \
  -extfile <(printf 'basicConstraints = CA:false\nkeyUsage = critical, digitalSignature\nextendedKeyUsage = serverAuth\nsubjectAltName = %s\n' "$alt")
rm server.csr

echo "Certificates in $dir, for $alt"
