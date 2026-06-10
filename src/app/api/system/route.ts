import { NextResponse } from 'next/server';
import { execSync } from 'child_process';
import os from 'os';

let previousCpu = os.cpus().map((cpu) => cpu.times);

const getCpuUsage = () => {
  const currentCpu = os.cpus().map((cpu) => cpu.times);
  let idle = 0;
  let total = 0;

  currentCpu.forEach((times, index) => {
    const previous = previousCpu[index] ?? times;
    const idleDelta = times.idle - previous.idle;
    const totalDelta = Object.values(times).reduce((sum, value) => sum + value, 0) - Object.values(previous).reduce((sum, value) => sum + value, 0);

    idle += idleDelta;
    total += totalDelta;
  });

  previousCpu = currentCpu;
  if (total <= 0) return 0;

  return Math.max(0, Math.min(100, Math.round((1 - idle / total) * 100)));
};

const formatGb = (bytes: number) => `${(bytes / 1024 / 1024 / 1024).toFixed(1)}G`;
const formatMib = (bytes: number) => `${Math.round(bytes / 1024 / 1024)}MiB`;

const runPowerShellJson = <T>(command: string, fallback: T): T => {
  try {
    const output = execSync(`powershell -NoProfile -Command "${command.replace(/"/g, '`"')} | ConvertTo-Json -Depth 4"`, {
      encoding: 'utf8',
      timeout: 2500,
      windowsHide: true,
    }).trim();

    if (!output) return fallback;
    return JSON.parse(output) as T;
  } catch {
    return fallback;
  }
};

const getDeviceDetails = () => {
  const gpu = runPowerShellJson<Array<{ Name?: string }> | { Name?: string }>(
    'Get-CimInstance Win32_VideoController | Select-Object -First 1 Name',
    [],
  );
  const disks = runPowerShellJson<Array<{ DeviceID?: string; Size?: number; FreeSpace?: number; VolumeName?: string }>>(
    'Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3" | Select-Object DeviceID,VolumeName,Size,FreeSpace',
    [],
  );

  return {
    gpu: Array.isArray(gpu) ? gpu[0]?.Name : gpu.Name,
    disks: (Array.isArray(disks) ? disks : [disks]).filter(Boolean).map((disk) => {
      const size = Number(disk.Size ?? 0);
      const free = Number(disk.FreeSpace ?? 0);
      const used = Math.max(0, size - free);

      return {
        id: disk.DeviceID ?? '-',
        name: disk.VolumeName ?? '',
        used: formatGb(used),
        total: formatGb(size),
        percent: size ? Math.round((used / size) * 100) : 0,
      };
    }),
  };
};

export async function GET() {
  const totalMemory = os.totalmem();
  const freeMemory = os.freemem();
  const usedMemory = totalMemory - freeMemory;
  const cpuInfo = os.cpus()[0];
  const details = getDeviceDetails();

  return NextResponse.json({
    cpu: getCpuUsage(),
    cpuModel: cpuInfo?.model?.replace(/\s+/g, ' ').trim() ?? 'Unknown CPU',
    cpuSpeed: cpuInfo ? `${(cpuInfo.speed / 1000).toFixed(2)} GHz` : '0.00 GHz',
    cores: os.cpus().length,
    platform: os.platform(),
    release: os.release(),
    hostname: os.hostname(),
    uptime: os.uptime(),
    ramUsed: formatGb(usedMemory),
    ramTotal: formatGb(totalMemory),
    memory: `${formatMib(usedMemory)} / ${formatMib(totalMemory)}`,
    ramPercent: Math.round((usedMemory / totalMemory) * 100),
    gpu: details.gpu ?? 'Unknown GPU',
    disks: details.disks,
  });
}
