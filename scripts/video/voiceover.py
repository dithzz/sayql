"""Reads the lines in voiceover.json with Kokoro (an open-source, Apache-2.0 voice model) into
one WAV per line, for render.mjs (VO_DIR=<out dir>).

    uv venv scripts/video/out/kokoro-venv --python 3.13
    uv pip install --python scripts/video/out/kokoro-venv/bin/python "kokoro>=0.9.4" soundfile \
      "en_core_web_sm @ https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl"
    scripts/video/out/kokoro-venv/bin/python scripts/video/voiceover.py VOICE OUT_DIR [--speed 1.0]

VOICE is a Kokoro voice id, such as af_heart, am_michael or bf_emma. The model (~330 MB)
downloads from Hugging Face on first use.
"""

import argparse
import json
import os
import tempfile

import espeakng_loader
import numpy as np
import soundfile as sf

# espeak-ng is Kokoro's fallback for words not in its dictionary. It keeps its data path in a
# 160-byte buffer and quietly falls back to where it was built when the path is longer, as
# it is in a deep checkout; so it gets a short link to its data instead.
_data = espeakng_loader.get_data_path()
if len(_data) > 150:
    _short = os.path.join(tempfile.gettempdir(), "sayql-espeak-ng-data")
    if os.path.realpath(_short) != os.path.realpath(_data):
        if os.path.lexists(_short):
            os.remove(_short)
        os.symlink(_data, _short)
    espeakng_loader.get_data_path = lambda: _short
    _data = _short
os.environ["ESPEAK_DATA_PATH"] = _data  # for the calls that don't pass a path

from kokoro import KPipeline  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument("voice")
ap.add_argument("out")
ap.add_argument("--speed", type=float, default=1.0)
a = ap.parse_args()

lines = json.load(open(os.path.join(os.path.dirname(__file__), "voiceover.json")))
pipeline = KPipeline(lang_code=a.voice[0], repo_id="hexgrad/Kokoro-82M")
os.makedirs(a.out, exist_ok=True)
for line in lines:
    audio = np.concatenate([np.asarray(r.audio) for r in pipeline(line["text"], voice=a.voice, speed=a.speed)])
    sf.write(os.path.join(a.out, f"{line['id']}.wav"), audio, 24000)
    print(f"{line['id']}: {len(audio) / 24000:.1f} s")
