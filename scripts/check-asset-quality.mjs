// Compares the compressed assets against the originals from git, at the size
// they are actually displayed. Judging a 256px avatar by diffing it at 1254px
// would measure the downscale, not the compression.

import sharp from 'sharp';
import { execSync } from 'node:child_process';

const cases = [
  { file: 'public/profile-avatar.png', renderAt: 88 },
  { file: 'public/wallpaper-anime-moon.jpg', renderAt: 1920 },
];

for (const { file, renderAt } of cases) {
  // Original bytes straight from the last commit, never written to disk.
  const original = execSync(`git show HEAD:${file}`, { encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });

  const toRaw = (input) =>
    sharp(input).resize({ width: renderAt, height: renderAt, fit: 'inside' }).removeAlpha().raw().toBuffer({ resolveWithObject: true });

  const a = await toRaw(original);
  const b = await toRaw(file);

  if (a.data.length !== b.data.length) {
    console.log(`${file}: dimensions differ after normalising, cannot compare`);
    continue;
  }

  let sum = 0;
  let worst = 0;
  for (let i = 0; i < a.data.length; i++) {
    const d = Math.abs(a.data[i] - b.data[i]);
    sum += d;
    if (d > worst) worst = d;
  }
  const mae = sum / a.data.length;
  // Rough rule of thumb: under ~2/255 is invisible, under ~5 is hard to spot.
  const verdict = mae < 2 ? 'imperceptible' : mae < 5 ? 'barely visible' : 'VISIBLE — reconsider';
  console.log(
    `${file.padEnd(34)} at ${renderAt}px  mean error ${mae.toFixed(2)}/255  worst pixel ${worst}  → ${verdict}`,
  );
}
