#!/usr/bin/env python3
import json
import re
import sys

payload = json.load(sys.stdin)
output = payload.get("output", "")
# Simulate a "repair" by removing loss markers in Typst or LaTeX comments
clean = re.sub(r"/\*\s*tylax:loss:[^*]*\*/\s*", "", output)
clean = re.sub(r"^%\\s*tylax:loss:.*$", "", clean, flags=re.MULTILINE)
print(clean.strip())
