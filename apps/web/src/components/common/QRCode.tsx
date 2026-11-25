import { useMemo } from 'react';
import qrcode from 'qrcode-generator';
import { cn } from '../../lib/utils';

/**
 * Tiny inline QR renderer. We only need small payloads (openmesh:// invites are
 * < 300 chars), so byte mode with M-level error correction fits easily. Drawing
 * the matrix ourselves keeps the QR crisp at any size and styled for the app.
 */
export function QRCode({
  value,
  size = 200,
  className,
  label = 'QR code',
  quietZone = 3,
}: {
  value: string;
  size?: number;
  className?: string;
  label?: string;
  quietZone?: number;
}) {
  const { path, dimension } = useMemo(() => {
    try {
      const qr = qrcode(0, 'M');
      qr.addData(value);
      qr.make();
      const count = qr.getModuleCount();
      const total = count + quietZone * 2;
      let d = '';
      for (let row = 0; row < count; row += 1) {
        for (let col = 0; col < count; col += 1) {
          if (!qr.isDark(row, col)) continue;
          const x = col + quietZone;
          const y = row + quietZone;
          d += `M${x} ${y}h1v1h-1z`;
        }
      }
      return { path: d, dimension: total };
    } catch {
      return { path: '', dimension: 0 };
    }
  }, [value, quietZone]);

  if (!path) {
    return (
      <div
        className={cn('grid place-items-center rounded-xl border border-white/10 bg-ink-900 text-2xs text-slate-500', className)}
        style={{ width: size, height: size }}
      >
        QR unavailable
      </div>
    );
  }

  return (
    <div className={cn('relative inline-block rounded-2xl bg-slate-50 p-1 shadow-[0_10px_40px_-18px_rgba(0,0,0,0.9)]', className)}>
      <svg
        role="img"
        aria-label={label}
        width={size}
        height={size}
        viewBox={`0 0 ${dimension} ${dimension}`}
        shapeRendering="crispEdges"
        className="block rounded-xl"
      >
        <rect width={dimension} height={dimension} fill="#f8fafc" />
        <path d={path} fill="#0b1017" />
      </svg>
    </div>
  );
}
