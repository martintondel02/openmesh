/**
 * Geo helpers for the map screen. All pure so the fallback style and accuracy
 * circles are unit-testable without a WebGL context.
 */

export interface LatLon {
  lat: number;
  lon: number;
}

export interface Bounds {
  west: number;
  south: number;
  east: number;
  north: number;
}

const EARTH_RADIUS_M = 6_371_000;

/** Destination point on a sphere; returns [lon, lat] (GeoJSON order). */
export function destinationPoint(origin: LatLon, bearingDeg: number, distanceM: number): [number, number] {
  const bearing = (bearingDeg * Math.PI) / 180;
  const lat1 = (origin.lat * Math.PI) / 180;
  const lon1 = (origin.lon * Math.PI) / 180;
  const angular = distanceM / EARTH_RADIUS_M;
  const lat2 = Math.asin(
    Math.sin(lat1) * Math.cos(angular) + Math.cos(lat1) * Math.sin(angular) * Math.cos(bearing),
  );
  const lon2 =
    lon1 +
    Math.atan2(
      Math.sin(bearing) * Math.sin(angular) * Math.cos(lat1),
      Math.cos(angular) - Math.sin(lat1) * Math.sin(lat2),
    );
  return [(lon2 * 180) / Math.PI, (lat2 * 180) / Math.PI];
}

/**
 * Closed ring approximating a circle of `radiusM` around a point.
 * Used for the own-position accuracy halo.
 */
export function geoCirclePolygon(center: LatLon, radiusM: number, steps = 48): [number, number][] {
  const safeRadius = Math.max(1, radiusM);
  const count = Math.max(8, Math.min(256, Math.round(steps)));
  const ring: [number, number][] = [];
  for (let i = 0; i <= count; i += 1) {
    ring.push(destinationPoint(center, (i * 360) / count, safeRadius));
  }
  return ring;
}

/** Axis-aligned bounds for a set of positions, padded by `padDeg` degrees. */
export function boundsForPositions(positions: LatLon[], padDeg = 0.01): Bounds | null {
  if (positions.length === 0) return null;
  let west = Infinity;
  let east = -Infinity;
  let south = Infinity;
  let north = -Infinity;
  for (const position of positions) {
    if (!Number.isFinite(position.lat) || !Number.isFinite(position.lon)) continue;
    west = Math.min(west, position.lon);
    east = Math.max(east, position.lon);
    south = Math.min(south, position.lat);
    north = Math.max(north, position.lat);
  }
  if (!Number.isFinite(west) || !Number.isFinite(south)) return null;
  return {
    west: west - padDeg,
    east: east + padDeg,
    south: south - padDeg,
    north: north + padDeg,
  };
}

/** GeoJSON FeatureCollection helpers (structurally typed to avoid a maplibre import here). */
export interface FeatureCollectionLike {
  type: 'FeatureCollection';
  features: Array<{
    type: 'Feature';
    properties: Record<string, unknown>;
    geometry: { type: 'LineString'; coordinates: [number, number][] } | { type: 'Polygon'; coordinates: [number, number][][] } | { type: 'Point'; coordinates: [number, number] };
  }>;
}

export function lineFeatureCollection(lines: Array<{ coordinates: [number, number][]; properties?: Record<string, unknown> }>): FeatureCollectionLike {
  return {
    type: 'FeatureCollection',
    features: lines.map((line) => ({
      type: 'Feature',
      properties: line.properties ?? {},
      geometry: { type: 'LineString', coordinates: line.coordinates },
    })),
  };
}

export function polygonFeatureCollection(
  rings: Array<{ ring: [number, number][]; properties?: Record<string, unknown> }>,
): FeatureCollectionLike {
  return {
    type: 'FeatureCollection',
    features: rings.map(({ ring, properties }) => ({
      type: 'Feature',
      properties: properties ?? {},
      geometry: { type: 'Polygon', coordinates: [ring] },
    })),
  };
}

export function circleFeatureCollection(center: LatLon, radiusM: number): FeatureCollectionLike {
  return polygonFeatureCollection([{ ring: geoCirclePolygon(center, radiusM) }]);
}

/** Distance in metres between two points (haversine). */
export function distanceMeters(a: LatLon, b: LatLon): number {
  const dLat = ((b.lat - a.lat) * Math.PI) / 180;
  const dLon = ((b.lon - a.lon) * Math.PI) / 180;
  const lat1 = (a.lat * Math.PI) / 180;
  const lat2 = (b.lat * Math.PI) / 180;
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(lat1) * Math.cos(lat2) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_RADIUS_M * Math.asin(Math.sqrt(h));
}
