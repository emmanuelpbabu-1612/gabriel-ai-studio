import { useEffect, useRef, type FC } from 'react';

const SVG_W = 400;
const SVG_H = 200;
const PAD_TOP = 20;
const PAD_BOTTOM = 10;

type Coord = { x: number; y: number };

function mapDataToCoords(data: number[], loValue: number, hiValue: number): Coord[] {
  if (data.length === 0) return [];
  const stepX = data.length > 1 ? SVG_W / (data.length - 1) : 0;
  const usableH = SVG_H - PAD_TOP - PAD_BOTTOM;
  const span = hiValue - loValue;
  return data.map((val, i) => ({
    x: i * stepX,
    y: span > 0
      ? PAD_TOP + (usableH - ((val - loValue) / span) * usableH)
      : PAD_TOP + usableH / 2, // genuinely constant data: honest flat midline
  }));
}

function createLinePath(coords: Coord[]): string {
  return coords.map((c, i) => (i === 0 ? `M ${c.x},${c.y}` : `L ${c.x},${c.y}`)).join(' ');
}

function createAreaPath(coords: Coord[]): string {
  return `${createLinePath(coords)} L ${SVG_W},${SVG_H} L 0,${SVG_H} Z`;
}

function renderPoints(
  coords: Coord[],
  groupEl: SVGGElement,
  colorClass: string
) {
  const existing = groupEl.querySelectorAll('circle');
  coords.forEach((c, i) => {
    let circle = existing[i] as SVGCircleElement | undefined;
    if (!circle) {
      circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle') as SVGCircleElement;
      circle.setAttribute('class', `chart-point ${colorClass}`);
      circle.setAttribute('r', '4');
      circle.setAttribute('cx', String(c.x));
      circle.setAttribute('cy', String(c.y));
      groupEl.appendChild(circle);
    }
    const captured = circle;
    requestAnimationFrame(() => {
      captured.setAttribute('cx', String(c.x));
      captured.setAttribute('cy', String(c.y));
    });
  });
  for (let i = coords.length; i < existing.length; i++) {
    existing[i].remove();
  }
}

export interface DualGlowingChartProps {
  primaryData: number[];
  secondaryData: number[];
  primaryColor?: string;
  secondaryColor?: string;
  cardBg?: string;
  height?: number;
  className?: string;
}

export const DualGlowingChart: FC<DualGlowingChartProps> = ({
  primaryData,
  secondaryData,
  primaryColor = '#1F60FF',
  secondaryColor = '#3FA76B',
  cardBg = '#171717',
  height = 160,
  className = '',
}) => {
  const svgRef = useRef<SVGSVGElement>(null);

  const styleId = useRef(`dgc-${Math.random().toString(36).slice(2)}`);

  useEffect(() => {
    const id = styleId.current;
    const existing = document.getElementById(id);
    if (!existing) {
      const tag = document.createElement('style');
      tag.id = id;
      tag.textContent = `
        .${id} .chart-line,
        .${id} .chart-area,
        .${id} .chart-point {
          transition: d 0.7s cubic-bezier(0.4, 0, 0.2, 1),
                      cx 0.7s cubic-bezier(0.4, 0, 0.2, 1),
                      cy 0.7s cubic-bezier(0.4, 0, 0.2, 1);
        }
        .${id} .chart-line {
          fill: none;
          stroke-width: 2.5;
          stroke-linecap: round;
          stroke-linejoin: round;
        }
        .${id} .chart-area { stroke: none; }
        .${id} .chart-point {
          fill: ${cardBg};
          stroke-width: 2.5;
        }
        .${id} .chart-line.primary-line {
          stroke: ${primaryColor};
        }
        .${id} .chart-area.primary-area { fill: url(#${id}-grad-primary); }
        .${id} .chart-point.primary-pt { stroke: ${primaryColor}; }
        .${id} .chart-line.secondary-line {
          stroke: ${secondaryColor};
        }
        .${id} .chart-area.secondary-area { fill: url(#${id}-grad-secondary); }
        .${id} .chart-point.secondary-pt { stroke: ${secondaryColor}; }
      `;
      document.head.appendChild(tag);
    }
    return () => {
      document.getElementById(id)?.remove();
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [primaryColor, secondaryColor, cardBg]);

  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;

    // Autoscale to the visible data range (with padding) instead of [0, max]:
    // real sub-GB RAM fluctuations are sub-pixel on a zero-based scale and
    // render as a fake flat line. Genuinely constant data still draws flat —
    // at mid-chart, which is the honest signal for "no variation".
    const all = [...primaryData, ...secondaryData].filter((v) => Number.isFinite(v));
    let lo = all.length > 0 ? Math.min(...all) : 0;
    let hi = all.length > 0 ? Math.max(...all) : 1;
    const pad = (hi - lo) * 0.15 || Math.max(Math.abs(hi) * 0.05, 1e-6);
    lo -= pad;
    hi += pad;

    const coordsPrimary = mapDataToCoords(primaryData, lo, hi);
    const coordsSecondary = mapDataToCoords(secondaryData, lo, hi);

    const linePrimary = svg.getElementById('line-primary') as SVGPathElement | null;
    const areaPrimary = svg.getElementById('area-primary') as SVGPathElement | null;
    const pointsPrimary = svg.getElementById('points-primary') as SVGGElement | null;
    const lineSecondary = svg.getElementById('line-secondary') as SVGPathElement | null;
    const areaSecondary = svg.getElementById('area-secondary') as SVGPathElement | null;
    const pointsSecondary = svg.getElementById('points-secondary') as SVGGElement | null;

    if (lineSecondary) lineSecondary.setAttribute('d', createLinePath(coordsSecondary));
    if (areaSecondary) areaSecondary.setAttribute('d', createAreaPath(coordsSecondary));
    if (pointsSecondary) renderPoints(coordsSecondary, pointsSecondary, 'secondary-pt');

    if (linePrimary) linePrimary.setAttribute('d', createLinePath(coordsPrimary));
    if (areaPrimary) areaPrimary.setAttribute('d', createAreaPath(coordsPrimary));
    if (pointsPrimary) renderPoints(coordsPrimary, pointsPrimary, 'primary-pt');
  }, [primaryData, secondaryData]);

  const id = styleId.current;

  return (
    <div className={`w-full ${className}`} style={{ height }}>
      <svg
        ref={svgRef}
        viewBox={`0 0 ${SVG_W} ${SVG_H}`}
        preserveAspectRatio="none"
        className={`w-full h-full overflow-visible ${id}`}
      >
        <defs>
          <linearGradient id={`${id}-grad-secondary`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={secondaryColor} stopOpacity={0.25} />
            <stop offset="100%" stopColor={secondaryColor} stopOpacity={0} />
          </linearGradient>
          <linearGradient id={`${id}-grad-primary`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={primaryColor} stopOpacity={0.3} />
            <stop offset="100%" stopColor={primaryColor} stopOpacity={0} />
          </linearGradient>
        </defs>

        {/* Background (secondary) layer */}
        <path id="area-secondary" className="chart-area secondary-area" d="" />
        <path id="line-secondary" className="chart-line secondary-line" d="" />
        <g id="points-secondary" />

        {/* Foreground (primary) layer */}
        <path id="area-primary" className="chart-area primary-area" d="" />
        <path id="line-primary" className="chart-line primary-line" d="" />
        <g id="points-primary" />
      </svg>
    </div>
  );
};