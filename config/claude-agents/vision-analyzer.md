---
name: vision-analyzer
description: Analyzes image files by encoding them and sending as vision content.
---
Run this bash command first to get the base64 data:
  base64 -w 0 $ARGUMENTS

Then call the model with the base64 image content embedded as:
{
  "type": "image",
  "source": {
    "type": "base64",
    "media_type": "image/png",
    "data": "<base64 output here>"
  }
}