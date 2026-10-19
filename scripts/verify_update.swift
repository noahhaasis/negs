// Verify against the public key shipped in the app, not merely the CI private
// key used to sign. This catches mismatched secrets before publication.
import CryptoKit
import Foundation

guard CommandLine.arguments.count == 4,
      let key = Data(base64Encoded: CommandLine.arguments[1]),
      let signature = Data(base64Encoded: CommandLine.arguments[2]) else {
    fatalError("Usage: swift verify_update.swift PUBLIC_KEY SIGNATURE ARCHIVE")
}
let publicKey = try Curve25519.Signing.PublicKey(rawRepresentation: key)
let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[3]), options: .mappedIfSafe)
guard publicKey.isValidSignature(signature, for: data) else {
    fputs("Update signature does not match the app's public key.\n", stderr)
    exit(1)
}
print("Update signature verified against the embedded public key.")
