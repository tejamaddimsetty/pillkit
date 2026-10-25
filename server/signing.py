"""Ed25519 signing of licences and feeds, in the `barkit1.<payload>.<sig>`
format the host verifies (docs/PROTOCOL.md §1)."""
import base64
import json
import os
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

PREFIX = "barkit1"


def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


class Signer:
    def __init__(self, key: Ed25519PrivateKey):
        self._key = key

    @classmethod
    def load_or_create(cls, path):
        path = Path(path)
        if not path.is_absolute():
            path = Path(__file__).parent / path
        if path.exists():
            key = serialization.load_pem_private_key(path.read_bytes(), password=None)
        else:
            key = Ed25519PrivateKey.generate()
            path.parent.mkdir(parents=True, exist_ok=True)
            pem = key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())
            path.write_bytes(pem)
            os.chmod(path, 0o600)
        return cls(key)

    @property
    def public_key_b64(self) -> str:
        raw = self._key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        return b64url(raw)

    def seal(self, payload: dict) -> str:
        body = b64url(json.dumps(payload, separators=(",", ":")).encode())
        signed = f"{PREFIX}.{body}"
        return f"{signed}.{b64url(self._key.sign(signed.encode()))}"
