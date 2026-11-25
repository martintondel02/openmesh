import { describe, expect, it } from 'vitest';
import {
  boundsForPositions,
  circleFeatureCollection,
  destinationPoint,
  distanceMeters,
  geoCirclePolygon,
  lineFeatureCollection,
  polygonFeatureCollection,
} from '../geo';
import { createOfflineMapStyle, MAP_PLACE_LABELS } from '../map-style';

describe('geo: circle polygons', () => {
  const oslo = { lat: 59.913, lon: 10.752 };

  it('returns a closed ring with the requested number of segments', () => {
    const ring = geoCirclePolygon(oslo, 100, 32);
    expect(ring).toHaveLength(33);
    expect(ring[0]).toEqual(ring[ring.length - 1]);
  });

  it('places every vertex roughly on the requested radius', () => {
    const radius = 250;
    const ring = geoCirclePolygon(oslo, radius, 24);
    for (const point of ring.slice(0, -1)) {
      const distance = distanceMeters(oslo, { lat: point[1], lon: point[0] });
      expect(Math.abs(distance - radius)).toBeLessThan(radius * 0.01);
    }
  });

  it('clamps tiny radii and step counts', () => {
    expect(geoCirclePolygon(oslo, 0, 1)).toHaveLength(9);
    expect(geoCirclePolygon(oslo, -50)).toHaveLength(49);
  });

  it('moves east when bearing is 90°', () => {
    const [lon] = destinationPoint(oslo, 90, 1000);
    expect(lon).toBeGreaterThan(oslo.lon);
  });
});

describe('geo: bounds and feature collections', () => {
  it('computes padded bounds', () => {
    const bounds = boundsForPositions(
      [
        { lat: 59.9, lon: 10.7 },
        { lat: 60.0, lon: 10.8 },
      ],
      0.01,
    );
    expect(bounds).toEqual({ west: 10.69, east: 10.81, south: 59.89, north: 60.01 });
  });

  it('returns null for empty or invalid input', () => {
    expect(boundsForPositions([])).toBeNull();
    expect(boundsForPositions([{ lat: Number.NaN, lon: Number.NaN }])).toBeNull();
  });

  it('builds feature collections for lines, polygons and circles', () => {
    const lines = lineFeatureCollection([{ coordinates: [[10.7, 59.9], [10.8, 60.0]] }]);
    expect(lines.type).toBe('FeatureCollection');
    expect(lines.features[0]!.geometry.type).toBe('LineString');

    const polygons = polygonFeatureCollection([{ ring: [[0, 0], [1, 0], [1, 1], [0, 0]] }]);
    expect(polygons.features[0]!.geometry.type).toBe('Polygon');

    const circles = circleFeatureCollection({ lat: 59.9, lon: 10.7 }, 50);
    expect(circles.features[0]!.geometry.type).toBe('Polygon');
  });

  it('measures distances in metres', () => {
    const oslo = { lat: 59.913, lon: 10.752 };
    const anne = { lat: 59.914, lon: 10.753 };
    const distance = distanceMeters(oslo, anne);
    expect(distance).toBeGreaterThan(100);
    expect(distance).toBeLessThan(200);
    expect(distanceMeters(oslo, oslo)).toBe(0);
  });
});

describe('map style: offline fallback', () => {
  const style = createOfflineMapStyle();

  it('declares sources without remote tile dependencies by default', () => {
    expect(style.version).toBe(8);
    expect(style.sources.water).toBeTruthy();
    expect(style.sources['roads-major']).toBeTruthy();
    expect(style.sources['own-accuracy']).toBeTruthy();
    // The raster source exists but its layer is hidden until the user opts in.
    const raster = style.layers.find((layer) => layer.id === 'osm-raster');
    expect(raster?.type).toBe('raster');
    expect(raster?.layout?.visibility).toBe('none');
  });

  it('hides the raster layer when online tiles are requested', () => {
    const online = createOfflineMapStyle({ onlineTiles: true });
    const raster = online.layers.find((layer) => layer.id === 'osm-raster');
    expect(raster?.layout?.visibility).toBe('visible');
    const water = online.layers.find((layer) => layer.id === 'water-fill');
    expect(water?.layout?.visibility).toBe('none');
  });

  it('ships place labels for the DOM marker layer', () => {
    expect(MAP_PLACE_LABELS.length).toBeGreaterThan(5);
    expect(MAP_PLACE_LABELS.some((place) => place.name === 'Oslo')).toBe(true);
    expect(MAP_PLACE_LABELS.every((place) => Number.isFinite(place.lat) && Number.isFinite(place.lon))).toBe(true);
  });
});
