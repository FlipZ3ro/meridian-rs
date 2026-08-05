// One-off asset compression. Both files were stored at capture resolution and
// served at a fraction of it: the avatar is 1254px square for an 88px slot, and
// the wallpaper is 4368px wide for a background that the terminal covers
// entirely and the lock screen blurs by 8px.
//
// Originals stay in git history, so `git checkout <sha> -- public/…` restores
// them if a size or quality choice turns out wrong.

import sharp from 'sharp';
import { statSync, renameSync } from 'node:fs';

const mb = (p) => (statSync(p).size / 1024 / 1024).toFixed(2);
const kb = (p) => Math.round(statSync(p).size / 1024);

const jobs = [
  {
    file: 'public/profile-avatar.png',
    // 88px slot; 256 covers 2x displays and browser zoom with room to spare.
    targetWidth: 256,
    run: (input, out) => sharp(input).resize(256, 256, { fit: 'cover' }).png({ compressionLevel: 9, palette: true }).toFile(out),
  },
  {
    file: 'public/wallpaper-anime-moon.jpg',
    // 2560 wide still covers a 1440p monitor at full-bleed; beyond that the
    // extra pixels are never sampled.
    targetWidth: 2560,
    run: (input, out) => sharp(input).resize({ width: 2560, withoutEnlargement: true }).jpeg({ quality: 80, mozjpeg: true }).toFile(out),
  },
];

for (const job of jobs) {
  const before = mb(job.file);
  const beforeMeta = await sharp(job.file).metadata();

  // Re-running would re-encode an already-compressed file and lose a little
  // more each time, so a file already at or below target is left alone. Drop a
  // new original in and this becomes safe to run again.
  if (beforeMeta.width <= job.targetWidth) {
    console.log(`${job.file.padEnd(34)} already ${beforeMeta.width}px wide (${before} MB) — skipped`);
    continue;
  }

  const tmp = `${job.file}.tmp`;
  await job.run(job.file, tmp);
  renameSync(tmp, job.file);
  const afterMeta = await sharp(job.file).metadata();
  console.log(
    `${job.file.padEnd(34)} ${beforeMeta.width}x${beforeMeta.height} ${before} MB` +
      `  ->  ${afterMeta.width}x${afterMeta.height} ${kb(job.file)} KB`,
  );
}
