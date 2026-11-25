/**
 * Bundled offline basemap for the map screen.
 *
 * MapLibre renders plain GeoJSON with zero remote requests: a schematic (but
 * recognisable) Oslo area with the fjord, Nordmarka forest, lakes and major
 * roads. No glyphs, sprites or tile servers are required, so the page keeps
 * working on an air-gapped machine. The map screen can layer online raster tiles
 * on top, with an automatic fallback if they fail to load.
 *
 * Runtime sources (`own-accuracy`, `history`, `links`) are declared empty here
 * and populated by the map screen.
 */

import type { StyleSpecification } from 'maplibre-gl';
import type { FeatureCollectionLike } from './geo';

type Ring = [number, number][];

const INK = {
  background: '#060c13',
  water: '#0a2032',
  waterEdge: '#174863',
  forest: '#0a1812',
  forestEdge: '#12251b',
  urban: '#0f1923',
  urbanEdge: '#182532',
  road: '#23364a',
  roadMinor: '#1a2836',
  graticule: 'rgba(148,163,184,0.06)',
};

const WATER: Ring = [
  [10.40, 59.40],
  [11.10, 59.40],
  [11.08, 59.56],
  [10.99, 59.67],
  [10.92, 59.74],
  [10.86, 59.80],
  [10.81, 59.855],
  [10.78, 59.88],
  [10.755, 59.898],
  [10.73, 59.906],
  [10.703, 59.902],
  [10.675, 59.892],
  [10.645, 59.875],
  [10.61, 59.852],
  [10.575, 59.822],
  [10.54, 59.785],
  [10.502, 59.735],
  [10.462, 59.67],
  [10.43, 59.59],
  [10.41, 59.50],
];

const ISLANDS: Ring[] = [
  // Nesodden peninsula (east of the fjord)
  [
    [10.805, 59.885],
    [10.862, 59.874],
    [10.91, 59.842],
    [10.938, 59.79],
    [10.925, 59.735],
    [10.892, 59.70],
    [10.862, 59.72],
    [10.842, 59.755],
    [10.83, 59.80],
    [10.812, 59.845],
  ],
  // Bygdøy
  [
    [10.672, 59.906],
    [10.71, 59.9],
    [10.72, 59.882],
    [10.695, 59.868],
    [10.665, 59.876],
    [10.658, 59.89],
  ],
  // Hovedøya / Lindøya / Nakholmen specks
  [
    [10.73, 59.889],
    [10.742, 59.886],
    [10.74, 59.878],
    [10.727, 59.879],
  ],
  [
    [10.754, 59.891],
    [10.766, 59.888],
    [10.762, 59.88],
    [10.75, 59.881],
  ],
  [
    [10.785, 59.891],
    [10.797, 59.888],
    [10.792, 59.879],
    [10.78, 59.88],
  ],
  // Snarøya
  [
    [10.588, 59.888],
    [10.615, 59.884],
    [10.612, 59.874],
    [10.592, 59.875],
  ],
];

const LAKES: Ring[] = [
  // Sognsvann
  [
    [10.723, 59.976],
    [10.731, 59.98],
    [10.743, 59.978],
    [10.746, 59.971],
    [10.737, 59.966],
    [10.726, 59.968],
  ],
  // Maridalsvannet
  [
    [10.762, 59.998],
    [10.772, 60.005],
    [10.789, 60.006],
    [10.803, 60.013],
    [10.815, 60.019],
    [10.824, 60.016],
    [10.82, 60.007],
    [10.803, 60.0],
    [10.79, 59.992],
    [10.775, 59.988],
    [10.766, 59.99],
  ],
  // Bogstadvannet
  [
    [10.628, 59.966],
    [10.64, 59.97],
    [10.65, 59.966],
    [10.648, 59.958],
    [10.635, 59.955],
    [10.625, 59.96],
  ],
  // Nøklevann
  [
    [10.855, 59.936],
    [10.872, 59.939],
    [10.885, 59.93],
    [10.881, 59.918],
    [10.866, 59.913],
    [10.852, 59.918],
    [10.845, 59.927],
  ],
  // Store Sandungen (Nordmarka)
  [
    [10.685, 60.045],
    [10.702, 60.05],
    [10.71, 60.043],
    [10.7, 60.034],
    [10.686, 60.034],
  ],
];

const FOREST: Ring[] = [
  [
    [10.545, 59.985],
    [10.62, 60.035],
    [10.72, 60.05],
    [10.81, 60.04],
    [10.865, 59.995],
    [10.85, 59.945],
    [10.79, 59.93],
    [10.7, 59.945],
    [10.615, 59.955],
    [10.56, 59.965],
  ],
  [
    [10.44, 59.99],
    [10.55, 60.005],
    [10.575, 59.965],
    [10.53, 59.925],
    [10.455, 59.93],
    [10.425, 59.955],
  ],
  [
    [10.795, 59.955],
    [10.86, 59.965],
    [10.9, 59.945],
    [10.885, 59.905],
    [10.845, 59.89],
    [10.81, 59.91],
  ],
];

const URBAN: Ring[] = [
  // Oslo city centre + inner ring
  [
    [10.712, 59.928],
    [10.75, 59.935],
    [10.79, 59.929],
    [10.808, 59.912],
    [10.795, 59.895],
    [10.76, 59.888],
    [10.724, 59.893],
    [10.703, 59.908],
  ],
  // Sandvika / Bærum
  [
    [10.492, 59.9],
    [10.545, 59.905],
    [10.56, 59.885],
    [10.522, 59.874],
    [10.488, 59.882],
  ],
  // Ski / east side
  [
    [10.855, 59.885],
    [10.895, 59.89],
    [10.905, 59.872],
    [10.862, 59.865],
  ],
];

const ROADS_MAJOR: Ring[] = [
  // E18 west → city → east
  [
    [10.45, 59.888],
    [10.52, 59.895],
    [10.6, 59.902],
    [10.67, 59.896],
    [10.712, 59.898],
    [10.76, 59.897],
    [10.82, 59.905],
    [10.9, 59.91],
  ],
  // E6 north → city → south
  [
    [10.79, 60.06],
    [10.8, 60.02],
    [10.782, 59.975],
    [10.772, 59.945],
    [10.762, 59.91],
    [10.775, 59.88],
    [10.825, 59.85],
    [10.88, 59.81],
  ],
  // Ring 3
  [
    [10.6, 59.935],
    [10.645, 59.947],
    [10.69, 59.953],
    [10.735, 59.951],
    [10.788, 59.947],
    [10.828, 59.938],
  ],
];

const ROADS_MINOR: Ring[] = [
  // Ring 2
  [
    [10.675, 59.92],
    [10.71, 59.93],
    [10.75, 59.933],
    [10.795, 59.92],
  ],
  // Bogstadveien / Majorstuen
  [
    [10.72, 59.918],
    [10.725, 59.928],
    [10.73, 59.938],
  ],
  // Nordmarka forest road
  [
    [10.74, 59.94],
    [10.73, 59.98],
    [10.71, 60.0],
    [10.66, 60.02],
    [10.6, 60.035],
  ],
  // Kolsås ridge road
  [
    [10.51, 59.92],
    [10.53, 59.93],
    [10.55, 59.94],
    [10.58, 59.95],
  ],
  // Sognsvann → city
  [
    [10.735, 59.972],
    [10.732, 59.952],
    [10.735, 59.935],
  ],
  // Bygdøy road
  [
    [10.685, 59.905],
    [10.7, 59.898],
    [10.704, 59.888],
  ],
];

function graticule(): [number, number][][] {
  const lines: [number, number][][] = [];
  for (let lon = 10.4; lon <= 11.05; lon += 0.05) {
    lines.push([
      [Number(lon.toFixed(3)), 59.85],
      [Number(lon.toFixed(3)), 60.08],
    ]);
  }
  for (let lat = 59.85; lat <= 60.081; lat += 0.025) {
    lines.push([
      [10.4, Number(lat.toFixed(4))],
      [11.05, Number(lat.toFixed(4))],
    ]);
  }
  return lines;
}

function polygon(rings: Ring[]): FeatureCollectionLike {
  return {
    type: 'FeatureCollection',
    features: rings.map((ring) => ({
      type: 'Feature',
      properties: {},
      geometry: { type: 'Polygon', coordinates: [ring] },
    })),
  };
}

function lines(rings: Ring[]): FeatureCollectionLike {
  return {
    type: 'FeatureCollection',
    features: rings.map((ring) => ({
      type: 'Feature',
      properties: {},
      geometry: { type: 'LineString', coordinates: ring },
    })),
  };
}

const empty: FeatureCollectionLike = { type: 'FeatureCollection', features: [] };

/** The complete bundled MapLibre style. */
export function createOfflineMapStyle(options: { onlineTiles?: boolean } = {}): StyleSpecification {
  const online = Boolean(options.onlineTiles);
  return {
    version: 8,
    name: 'OpenMesh offline schematic',
    sources: {
      'osm-tiles': {
        type: 'raster',
        tiles: ['https://tile.openstreetmap.org/{z}/{x}/{y}.png'],
        tileSize: 256,
        maxzoom: 19,
        attribution: '© OpenStreetMap contributors',
      },
      water: { type: 'geojson', data: polygon([WATER]) },
      islands: { type: 'geojson', data: polygon(ISLANDS) },
      lakes: { type: 'geojson', data: polygon(LAKES) },
      forest: { type: 'geojson', data: polygon(FOREST) },
      urban: { type: 'geojson', data: polygon(URBAN) },
      'roads-major': { type: 'geojson', data: lines(ROADS_MAJOR) },
      'roads-minor': { type: 'geojson', data: lines(ROADS_MINOR) },
      graticule: { type: 'geojson', data: lines(graticule()) },
      'own-accuracy': { type: 'geojson', data: empty },
      history: { type: 'geojson', data: empty },
      links: { type: 'geojson', data: empty },
    },
    layers: [
      { id: 'background', type: 'background', paint: { 'background-color': INK.background } },
      {
        id: 'osm-raster',
        type: 'raster',
        source: 'osm-tiles',
        layout: { visibility: online ? 'visible' : 'none' },
        paint: { 'raster-opacity': 0.85, 'raster-brightness-max': 0.7, 'raster-saturation': -0.4 },
      },
      {
        id: 'graticule',
        type: 'line',
        source: 'graticule',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'line-color': INK.graticule, 'line-width': 0.6 },
      },
      {
        id: 'forest-fill',
        type: 'fill',
        source: 'forest',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'fill-color': INK.forest },
      },
      {
        id: 'forest-edge',
        type: 'line',
        source: 'forest',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'line-color': INK.forestEdge, 'line-width': 0.8, 'line-opacity': 0.7 },
      },
      {
        id: 'urban-fill',
        type: 'fill',
        source: 'urban',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'fill-color': INK.urban },
      },
      {
        id: 'urban-edge',
        type: 'line',
        source: 'urban',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'line-color': INK.urbanEdge, 'line-width': 0.7, 'line-opacity': 0.8 },
      },
      {
        id: 'water-fill',
        type: 'fill',
        source: 'water',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'fill-color': INK.water },
      },
      {
        id: 'water-edge',
        type: 'line',
        source: 'water',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'line-color': INK.waterEdge, 'line-width': 1.6, 'line-opacity': 0.9 },
      },
      {
        id: 'island-fill',
        type: 'fill',
        source: 'islands',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'fill-color': INK.background },
      },
      {
        id: 'island-edge',
        type: 'line',
        source: 'islands',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'line-color': INK.waterEdge, 'line-width': 1.2, 'line-opacity': 0.8 },
      },
      {
        id: 'lakes-fill',
        type: 'fill',
        source: 'lakes',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'fill-color': INK.water },
      },
      {
        id: 'lakes-edge',
        type: 'line',
        source: 'lakes',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'line-color': INK.waterEdge, 'line-width': 0.9, 'line-opacity': 0.7 },
      },
      {
        id: 'roads-minor',
        type: 'line',
        source: 'roads-minor',
        layout: { visibility: online ? 'none' : 'visible', 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': INK.roadMinor, 'line-width': 1 },
      },
      {
        id: 'roads-major',
        type: 'line',
        source: 'roads-major',
        layout: { visibility: online ? 'none' : 'visible', 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': INK.road, 'line-width': 1.8 },
      },
      {
        id: 'links',
        type: 'line',
        source: 'links',
        layout: { visibility: online ? 'none' : 'visible', 'line-cap': 'round', 'line-join': 'round' },
        paint: {
          'line-color': '#38bdf8',
          'line-width': 1,
          'line-opacity': 0.22,
          'line-dasharray': [2, 4],
        },
      },
      {
        id: 'history',
        type: 'line',
        source: 'history',
        layout: { visibility: online ? 'none' : 'visible', 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': '#34d399', 'line-width': 1.6, 'line-opacity': 0.55, 'line-dasharray': [2, 2] },
      },
      {
        id: 'own-accuracy-fill',
        type: 'fill',
        source: 'own-accuracy',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'fill-color': '#38bdf8', 'fill-opacity': 0.08 },
      },
      {
        id: 'own-accuracy-line',
        type: 'line',
        source: 'own-accuracy',
        layout: { visibility: online ? 'none' : 'visible' },
        paint: { 'line-color': '#38bdf8', 'line-width': 1, 'line-opacity': 0.45 },
      },
    ],
  };
}

/** Place labels rendered as DOM markers (no glyph server needed). */
export const MAP_PLACE_LABELS: Array<{ name: string; lat: number; lon: number; minZoom?: number }> = [
  { name: 'Oslo', lat: 59.9, lon: 10.735, minZoom: 0 },
  { name: 'Sognsvann', lat: 59.963, lon: 10.746, minZoom: 10.5 },
  { name: 'Nordmarka', lat: 60.02, lon: 10.72, minZoom: 10.2 },
  { name: 'Holmenkollen', lat: 59.964, lon: 10.67, minZoom: 11.4 },
  { name: 'Bygdøy', lat: 59.902, lon: 10.685, minZoom: 11.6 },
  { name: 'Bærum', lat: 59.89, lon: 10.53, minZoom: 10.2 },
  { name: 'Nesodden', lat: 59.82, lon: 10.86, minZoom: 10.6 },
  { name: 'Grefsen', lat: 59.948, lon: 10.786, minZoom: 12.2 },
  { name: 'Maridalsvannet', lat: 60.0, lon: 10.79, minZoom: 11.4 },
];

export const MAP_ATTRIBUTION = 'Offline schematic · not survey-accurate';
