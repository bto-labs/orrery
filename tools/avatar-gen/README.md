# @orrery/avatar-gen

Stage 2 Subsystem A — per-repo character avatar generation pipeline for orrery.

Generates per-repo character-avatar sprites using base bot reference images and an image model.

## Usage

```bash
npm install
npm run derive    # derive avatar metadata from repo
npm run generate  # generate avatar sprites
npm run spike     # force-regenerate
```

## Assets

- `assets/bots/base/` — hand-authored base bot sprites (style anchors)
- `assets/agents/registry.json` — per-repo avatar registry
