// Optional SVG -> PNG previews; requires Node.js and the sharp package.
// SVGs remain the canonical fabrication/artwork sources.
const path = require('node:path');
const sharp = require('sharp');
const root = path.resolve(__dirname, '..');
async function main() {
  for (const [input, output, density] of [
    ['mechanical/led-front-artwork.svg', 'docs/led-front-preview.png', 140],
    ['docs/assembly-map.svg', 'docs/pcb-assembly-preview.png', 120],
    ['docs/led-wiring.svg', 'docs/led-wiring-preview.png', 72],
    ['docs/epaper-wiring.svg', 'docs/epaper-wiring-preview.png', 72],
  ]) {
    await sharp(path.join(root, input), { density }).png().toFile(path.join(root, output));
    process.stdout.write(`Rendered ${output}\n`);
  }
}
main().catch(error => { process.stderr.write(`${error}\n`); process.exitCode = 1; });
