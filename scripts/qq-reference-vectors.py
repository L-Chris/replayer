"""Regenerate non-secret test vectors: uv run --with libtakiyasha==2.1.1.post1 python scripts/qq-reference-vectors.py"""
import hashlib
import json
from base64 import b64encode
from libtakiyasha.qmc._qmcdataciphers import Mask128, HardenedRC4
from libtakiyasha.qmc._qmckeyciphers import QMCv2KeyEncryptV1, make_core_key

results = {}
for length in [256, 512]:
    key = bytes(i % 255 + 1 for i in range(length))
    cipher = Mask128.from_qmcv2_key256(key) if length == 256 else HardenedRC4(key)
    data = bytes(cipher.keystream('decrypt', 140000, 0))
    results[str(length)] = hashlib.sha256(data).hexdigest()
    if length == 256:
        results['ekey'] = b64encode(QMCv2KeyEncryptV1(make_core_key(106, 8)).encrypt(key)).decode()
print(json.dumps(results, indent=2))
