import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import maplibregl from 'maplibre-gl';
import type { GeoJSONSource, Map as MapLibreMap } from 'maplibre-gl';
import 'maplibre-gl/dist/maplibre-gl.css';
import {
  BatteryMedium,
  Crosshair,
  Footprints,
  Layers,
  LocateFixed,
  MapPin,
  Maximize2,
  Minus,
  Plus,
  Radio,
  Share2,
  ShieldCheck,
  Signal,
  TriangleAlert,
  X,
} from 'lucide-react';
import { useAppStore } from '../store/app';
import { selectNodesSorted } from '../store/selectors';
import type { NodeData, RoleClass } from '../types/api';
import { MAP_PLACE_LABELS, createOfflineMapStyle } from '../lib/map-style';
import { boundsForPositions, circleFeatureCollection, lineFeatureCollection, type FeatureCollectionLike } from '../lib/geo';
import { formatLastSeen, formatRole, cn, roleColor, shortenNodeId } from '../lib/utils';
import { Avatar, Badge, Button, IconButton, Tooltip } from '../components/ui/primitives';
import { BatteryIndicator, CapsIcons, RoleBadge, SignalBars } from '../components/ui/signals';
import { Modal } from '../components/ui/overlay';

const ROLE_ORDER: RoleClass[] = ['gateway', 'repeater', 'room_server', 'client', 'tracker'];
const ROLE_LABELS: Record<string, string> = {
  gateway: 'Gateway',
  repeater: 'Repeater',
  room_server: 'Room',
  client: 'Client',
  tracker: 'Tracker',
};

const EMPTY: FeatureCollectionLike = { type: 'FeatureCollection', features: [] };

function prefersReducedMotion(): boolean {
  return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
}

export function MapScreen() {
  const nodes = useAppStore((state) => state.nodes);
  const channels = useAppStore((state) => state.channels);
  const contacts = useAppStore((state) => state.contacts);
  const ownPosition = useAppStore((state) => state.ownPosition);
  const ownNodeId = useAppStore((state) => state.ownNodeId);
  const positionHistory = useAppStore((state) => state.positionHistory);
  const sharePosition = useAppStore((state) => state.sharePosition);
  const setOwnPositionFromGeo = useAppStore((state) => state.setOwnPositionFromGeo);
  const openThread = useAppStore((state) => state.openThread);
  const setScreen = useAppStore((state) => state.setScreen);
  const inspectNode = useAppStore((state) => state.inspectNode);
  const pushToast = useAppStore((state) => state.pushToast);

  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const markersRef = useRef<maplibregl.Marker[]>([]);
  const placeMarkersRef = useRef<Array<{ marker: maplibregl.Marker; minZoom: number }>>([]);
  const onlineRef = useRef(false);
  const [mapCreated, setMapCreated] = useState(false);
  const [mapReady, setMapReady] = useState(false);
  const autoFitRef = useRef(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [visibleRoles, setVisibleRoles] = useState<Set<RoleClass>>(new Set(ROLE_ORDER));
  const [showHistory, setShowHistory] = useState(false);
  const [onlineTiles, setOnlineTiles] = useState(false);
  const [tileError, setTileError] = useState(false);
  const [shareOpen, setShareOpen] = useState(false);
  const [locating, setLocating] = useState(false);

  const sortedNodes = useMemo(() => selectNodesSorted(nodes), [nodes]);
  const nodesWithPosition = useMemo(() => sortedNodes.filter((node) => node.position), [sortedNodes]);

  const visibleNodes = useMemo(
    () => nodesWithPosition.filter((node) => visibleRoles.has(node.class)),
    [nodesWithPosition, visibleRoles],
  );

  const selectedNode = selectedId ? nodes[selectedId] ?? null : null;
  const selectedIsSelf = selectedId === ownNodeId;

  /* ------------------------------ map instance ------------------------------ */

  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    const map = new maplibregl.Map({
      container: containerRef.current,
      style: createOfflineMapStyle(),
      center: [10.73, 59.945],
      zoom: 10.6,
      minZoom: 8,
      maxZoom: 17,
      attributionControl: false,
      dragRotate: false,
      pitchWithRotate: false,
      touchPitch: false,
      maxPitch: 0,
    });
    mapRef.current = map;
    setMapCreated(true);

    map.addControl(
      new maplibregl.AttributionControl({
        compact: true,
        customAttribution: 'Offline schematic',
      }),
      'bottom-right',
    );

    map.on('load', () => setMapReady(true));
    map.on('click', () => setSelectedId(null));
    map.on('error', (event) => {
      const sourceId = (event as unknown as { sourceId?: string }).sourceId;
      if (onlineRef.current && sourceId === 'osm-tiles') {
        onlineRef.current = false;
        setOnlineTiles(false);
        setTileError(true);
        pushToast({
          kind: 'warn',
          title: 'Online tiles unavailable',
          message: 'Falling back to the bundled offline basemap.',
        });
      }
    });

    const resizeObserver = new ResizeObserver(() => map.resize());
    resizeObserver.observe(containerRef.current);

    // Place names are DOM markers so no glyph server is needed.
    for (const place of MAP_PLACE_LABELS) {
      const element = document.createElement('div');
      element.className = 'map-place-label';
      element.textContent = place.name;
      element.setAttribute('aria-hidden', 'true');
      const marker = new maplibregl.Marker({ element, anchor: 'center' }).setLngLat([place.lon, place.lat]).addTo(map);
      placeMarkersRef.current.push({ marker, minZoom: place.minZoom ?? 0 });
    }
    const updatePlaceVisibility = () => {
      const zoom = map.getZoom();
      for (const entry of placeMarkersRef.current) {
        entry.marker.getElement().style.display = zoom >= entry.minZoom ? '' : 'none';
      }
    };
    updatePlaceVisibility();
    map.on('zoom', updatePlaceVisibility);

    return () => {
      resizeObserver.disconnect();
      for (const marker of placeMarkersRef.current) marker.marker.remove();
      placeMarkersRef.current = [];
      for (const marker of markersRef.current) marker.remove();
      markersRef.current = [];
      map.remove();
      mapRef.current = null;
      setMapReady(false);
      setMapCreated(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toLngLat = useCallback((lat: number, lon: number): [number, number] => [lon, lat], []);

  const flyTo = useCallback(
    (lat: number, lon: number, zoom = 13.2) => {
      mapRef.current?.flyTo({
        center: toLngLat(lat, lon),
        zoom,
        duration: prefersReducedMotion() ? 0 : 700,
      });
    },
    [toLngLat],
  );

  /* ------------------------------ node markers ------------------------------ */

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapCreated) return;
    for (const marker of markersRef.current) marker.remove();
    markersRef.current = [];

    for (const node of visibleNodes) {
      if (!node.position) continue;
      const element = document.createElement('button');
      element.type = 'button';
      element.className = 'map-node-marker';
      element.setAttribute('aria-label', `${node.name}, ${formatRole(node.class)}, ${node.hops} hops`);
      element.title = `${node.name} · ${formatRole(node.class)} · ${node.rssi ?? '—'} dBm · ${formatLastSeen(node.last_seen_ms)}`;
      const color = roleColor(node.class);
      const dot = document.createElement('span');
      dot.className = 'map-node-marker__dot';
      dot.style.backgroundColor = color;
      dot.style.boxShadow = `0 0 0 3px ${color}22, 0 0 14px ${color}66`;
      const label = document.createElement('span');
      label.className = 'map-node-marker__label';
      label.textContent = node.name;
      element.append(dot, label);
      element.dataset.alwaysLabel = 'true';
      if (node.node_id === selectedId) element.dataset.selected = 'true';
      element.addEventListener('click', (event) => {
        event.stopPropagation();
        setSelectedId(node.node_id);
      });
      const marker = new maplibregl.Marker({ element, anchor: 'left' })
        .setLngLat(toLngLat(node.position.lat, node.position.lon))
        .addTo(map);
      markersRef.current.push(marker);
    }

    if (ownPosition) {
      const element = document.createElement('div');
      element.style.cssText = 'position:relative;width:18px;height:18px;display:grid;place-items:center';
      element.innerHTML =
        '<span class="map-self-pulse"></span><span class="map-self-marker"></span><span class="map-node-marker__label" style="position:absolute;left:22px;top:50%;transform:translateY(-50%);opacity:1;white-space:nowrap">You</span>';
      element.setAttribute('title', 'Your position');
      element.addEventListener('click', (event) => {
        event.stopPropagation();
        setSelectedId(ownNodeId);
      });
      const marker = new maplibregl.Marker({ element, anchor: 'center' })
        .setLngLat(toLngLat(ownPosition.lat, ownPosition.lon))
        .addTo(map);
      markersRef.current.push(marker);
    }
  }, [mapCreated, visibleNodes, selectedId, ownPosition, ownNodeId, toLngLat]);

  /* ------------------------------ vector overlays ------------------------------ */

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    const accuracy = (map.getSource('own-accuracy') as GeoJSONSource | undefined);
    if (accuracy) {
      accuracy.setData(
        ownPosition
          ? (circleFeatureCollection(ownPosition, Math.max(10, ownPosition.accuracy_m ?? 15)))
          : EMPTY,
      );
    }

    const history = (map.getSource('history') as GeoJSONSource | undefined);
    if (history) {
      const historyNodeId = selectedId && selectedId !== ownNodeId ? selectedId : ownNodeId;
      const points = showHistory && historyNodeId ? positionHistory[historyNodeId] ?? [] : [];
      history.setData(
        lineFeatureCollection([
          { coordinates: points.map((point) => [point.lon, point.lat]) },
        ]),
      );
    }

    const links = (map.getSource('links') as GeoJSONSource | undefined);
    if (links) {
      const origin = ownPosition;
      const lines = origin
        ? visibleNodes
            .filter((node) => node.position)
            .map((node) => ({
              coordinates: [
                [origin.lon, origin.lat],
                [node.position!.lon, node.position!.lat],
              ] as [number, number][],
            }))
        : [];
      links.setData(lineFeatureCollection(lines));
    }
  }, [mapReady, ownPosition, ownNodeId, selectedId, showHistory, positionHistory, visibleNodes]);

  /* ------------------------------ tile toggle ------------------------------ */

  const fallbackLayerIds = useMemo(
    () => [
      'graticule',
      'forest-fill',
      'forest-edge',
      'urban-fill',
      'urban-edge',
      'water-fill',
      'water-edge',
      'island-fill',
      'island-edge',
      'lakes-fill',
      'lakes-edge',
      'roads-minor',
      'roads-major',
      'links',
      'history',
      'own-accuracy-fill',
      'own-accuracy-line',
    ],
    [],
  );

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapReady) return;
    onlineRef.current = onlineTiles;
    const setVisibility = (id: string, visible: boolean) => {
      try {
        map.setLayoutProperty(id, 'visibility', visible ? 'visible' : 'none');
      } catch {
        // Layer not ready yet; the style will be rebuilt correctly on next toggle.
      }
    };
    setVisibility('osm-raster', onlineTiles);
    for (const id of fallbackLayerIds) setVisibility(id, !onlineTiles);
  }, [mapReady, onlineTiles, fallbackLayerIds]);

  /* ------------------------------ actions ------------------------------ */

  const fitToNodes = useCallback(() => {
    const map = mapRef.current;
    const positions = [
      ...visibleNodes.map((node) => node.position).filter((position): position is NonNullable<typeof position> => Boolean(position)),
      ...(ownPosition ? [ownPosition] : []),
    ];
    const bounds = boundsForPositions(positions, 0.02);
    if (!map || !bounds) return;
    map.fitBounds(
      [
        [bounds.west, bounds.south],
        [bounds.east, bounds.north],
      ],
      { padding: { top: 90, bottom: 80, left: 60, right: 60 }, duration: prefersReducedMotion() ? 0 : 700 },
    );
  }, [visibleNodes, ownPosition]);

  // Frame the mesh once the basemap and nodes are ready (first meaningful view).
  const fitToNodesRef = useRef(fitToNodes);
  fitToNodesRef.current = fitToNodes;
  useEffect(() => {
    if (!mapCreated || autoFitRef.current || nodesWithPosition.length === 0) return;
    autoFitRef.current = true;
    const timer = setTimeout(() => fitToNodesRef.current(), 350);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mapCreated, nodesWithPosition.length]);

  const locateMe = useCallback(() => {
    if (!navigator.geolocation) {
      pushToast({ kind: 'warn', title: 'Geolocation unavailable', message: 'This browser has no location API.' });
      return;
    }
    setLocating(true);
    navigator.geolocation.getCurrentPosition(
      (result) => {
        const position = {
          lat: result.coords.latitude,
          lon: result.coords.longitude,
          alt_m: result.coords.altitude ?? null,
          accuracy_m: result.coords.accuracy ?? null,
          timestamp_ms: result.timestamp,
        };
        setOwnPositionFromGeo(position, position.accuracy_m);
        flyTo(position.lat, position.lon, 13.5);
        setLocating(false);
        pushToast({ kind: 'success', title: 'Location updated', message: 'Your position is now shown locally.' });
      },
      (error) => {
        setLocating(false);
        const demoFallback = ownPosition;
        if (demoFallback) {
          flyTo(demoFallback.lat, demoFallback.lon, 13);
          setSelectedId(ownNodeId);
          pushToast({
            kind: 'warn',
            title: 'Using last known position',
            message: `Browser location was blocked (${error.message}).`,
          });
        } else {
          pushToast({
            kind: 'error',
            title: 'No location fix',
            message: 'Allow location access, or share a position from the demo dataset.',
          });
        }
      },
      { enableHighAccuracy: true, timeout: 6_000, maximumAge: 30_000 },
    );
  }, [flyTo, ownNodeId, ownPosition, pushToast, setOwnPositionFromGeo]);

  const handleSharePosition = useCallback(
    async (target: { channelId?: string; peer?: string }) => {
      if (!ownPosition) {
        pushToast({
          kind: 'warn',
          title: 'No position yet',
          message: 'Use “Locate me” first, or enable position sharing on the bridge.',
        });
        return;
      }
      setShareOpen(false);
      await sharePosition(target);
      pushToast({
        kind: 'success',
        title: 'Position shared',
        message: target.channelId
          ? `Sent to #${channels[target.channelId]?.name ?? 'channel'}`
          : 'Sent as a direct position message',
      });
    },
    [channels, ownPosition, pushToast, sharePosition],
  );

  const toggleRole = (role: RoleClass) => {
    setVisibleRoles((current) => {
      const next = new Set(current);
      if (next.has(role)) {
        if (next.size > 1) next.delete(role);
      } else {
        next.add(role);
      }
      return next;
    });
  };

  const roleCounts = useMemo(() => {
    const counts: Partial<Record<RoleClass, number>> = {};
    for (const node of nodesWithPosition) counts[node.class] = (counts[node.class] ?? 0) + 1;
    return counts;
  }, [nodesWithPosition]);

  return (
    <div className="relative min-h-0 flex-1 overflow-hidden">
      <div ref={containerRef} className="h-full w-full overflow-hidden" role="application" aria-label="Mesh map of known nodes" />

      {/* top-left status card */}
      <div className="pointer-events-none absolute left-3 top-3 z-10 flex max-w-[calc(100%-1.5rem)] flex-col gap-2 md:left-5 md:top-4">
        <div className="glass pointer-events-auto rounded-xl border border-white/[0.08] px-3 py-2.5 shadow-panel">
          <div className="flex items-center gap-2">
            <span className="grid h-6 w-6 place-items-center rounded-md bg-emerald-400/10 text-emerald-300">
              <Radio className="h-3.5 w-3.5" aria-hidden="true" />
            </span>
            <div>
              <p className="text-xs font-semibold text-white">
                {visibleNodes.length} of {nodesWithPosition.length} positioned
              </p>
              <p className="text-3xs text-slate-500">
                {Object.keys(nodes).length} known nodes · {Object.keys(channels).length} channels
              </p>
            </div>
          </div>
          <div className="mt-2 flex flex-wrap gap-1">
            {ROLE_ORDER.filter((role) => roleCounts[role]).map((role) => {
              const active = visibleRoles.has(role);
              return (
                <button
                  key={role}
                  type="button"
                  onClick={() => toggleRole(role)}
                  aria-pressed={active}
                  className={cn(
                    'focus-ring inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-3xs font-medium transition-colors',
                    active
                      ? 'border-white/[0.12] bg-white/[0.07] text-slate-200'
                      : 'border-white/[0.05] bg-transparent text-slate-600 hover:text-slate-400',
                  )}
                >
                  <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: roleColor(role) }} />
                  {ROLE_LABELS[role]} <span className="tabular-nums opacity-60">{roleCounts[role]}</span>
                </button>
              );
            })}
          </div>
        </div>
        {true ? (
          <div
            className={
              'pointer-events-auto inline-flex w-fit items-center gap-1.5 rounded-full border px-2.5 py-1 text-3xs backdrop-blur ' +
              (tileError
                ? 'border-amber-400/25 bg-amber-400/[0.08] text-amber-200'
                : onlineTiles
                  ? 'border-sky-400/20 bg-sky-400/[0.06] text-sky-200/80'
                  : 'border-white/[0.08] bg-ink-950/70 text-slate-400')
            }
          >
            {tileError ? <TriangleAlert className="h-3 w-3 shrink-0" aria-hidden="true" /> : <Layers className="h-3 w-3 shrink-0" aria-hidden="true" />}
            {tileError
              ? 'Tiles unavailable — offline map'
              : onlineTiles
                ? 'Online tiles enabled'
                : 'Offline map · bundled basemap'}
            {tileError ? (
              <button
                type="button"
                aria-label="Dismiss tile warning"
                onClick={() => setTileError(false)}
                className="ml-0.5 grid h-4 w-4 place-items-center rounded hover:bg-amber-400/20"
              >
                <X className="h-2.5 w-2.5" />
              </button>
            ) : null}
          </div>
        ) : null}
      </div>

      {/* map controls */}
      <div className="absolute right-3 top-3 z-10 flex flex-col gap-1.5 md:right-4 md:top-4">
        <MapControl icon={Plus} label="Zoom in" onClick={() => mapRef.current?.zoomIn()} />
        <MapControl icon={Minus} label="Zoom out" onClick={() => mapRef.current?.zoomOut()} />
        <MapControl icon={Maximize2} label="Fit all nodes" onClick={fitToNodes} />
        <MapControl icon={locating ? Crosshair : LocateFixed} label="Locate me" onClick={locateMe} active={locating} spin={locating} />
        <MapControl icon={Share2} label="Share my position" onClick={() => setShareOpen(true)} />
        <MapControl icon={Footprints} label="Toggle position history" onClick={() => setShowHistory((value) => !value)} active={showHistory} />
        <MapControl
          icon={Layers}
          label={onlineTiles ? 'Use offline basemap' : 'Try online map tiles'}
          onClick={() => {
            setTileError(false);
            setOnlineTiles((value) => !value);
          }}
          active={onlineTiles}
        />
      </div>

      {/* selected node popover */}
      {selectedNode || (selectedIsSelf && ownPosition) ? (
        <div className="absolute right-3 top-3 z-20 w-[min(19rem,calc(100%-1.5rem))] md:right-14 md:top-4">
          {selectedIsSelf && ownPosition ? (
            <OwnPositionCard
              position={ownPosition}
              onClose={() => setSelectedId(null)}
              onShare={() => setShareOpen(true)}
              onLocate={locateMe}
            />
          ) : selectedNode ? (
            <NodeCard
              node={selectedNode}
              onClose={() => setSelectedId(null)}
              onCenter={() => {
                const position = selectedNode.position;
                if (position) flyTo(position.lat, position.lon);
              }}
              onMessage={() => {
                openThread({ kind: 'direct', id: selectedNode.node_id });
                setScreen('chats');
              }}
              onDetails={() => inspectNode(selectedNode.node_id)}
              isContact={Boolean(contacts[selectedNode.node_id])}
            />
          ) : null}
        </div>
      ) : null}

      {/* legend */}
      <div className="absolute bottom-3 left-3 z-10 hidden items-center gap-3 rounded-lg border border-white/[0.07] bg-ink-950/70 px-2.5 py-1.5 text-3xs text-slate-500 backdrop-blur sm:flex">
        {ROLE_ORDER.map((role) => (
          <span key={role} className="flex items-center gap-1">
            <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: roleColor(role) }} />
            {ROLE_LABELS[role]}
          </span>
        ))}
        <span className="ml-1 flex items-center gap-1 border-l border-white/[0.08] pl-2">
          <span className="h-1.5 w-1.5 rounded-full bg-sky-400" />
          You
        </span>
      </div>

      {/* mobile compact controls row */}
      <div className="absolute bottom-3 right-3 z-10 flex items-center gap-1.5 md:hidden">
        <button
          type="button"
          onClick={fitToNodes}
          className="focus-ring flex items-center gap-1.5 rounded-full border border-white/10 bg-ink-850/95 px-3 py-1.5 text-2xs font-medium text-slate-200 shadow-pop backdrop-blur"
        >
          <Maximize2 className="h-3 w-3" aria-hidden="true" />
          Fit nodes
        </button>
        <button
          type="button"
          onClick={() => setShareOpen(true)}
          className="focus-ring flex items-center gap-1.5 rounded-full bg-emerald-400 px-3 py-1.5 text-2xs font-semibold text-emerald-950 shadow-pop"
        >
          <Share2 className="h-3 w-3" aria-hidden="true" />
          Share
        </button>
      </div>

      <SharePositionModal
        open={shareOpen}
        onClose={() => setShareOpen(false)}
        channels={channels}
        contacts={contacts}
        onShare={(target) => void handleSharePosition(target)}
      />
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Small map pieces                                                   */
/* ------------------------------------------------------------------ */

function MapControl({
  icon: Icon,
  label,
  onClick,
  active = false,
  spin = false,
}: {
  icon: typeof Plus;
  label: string;
  onClick: () => void;
  active?: boolean;
  spin?: boolean;
}) {
  return (
    <Tooltip label={label}>
      <button
        type="button"
        onClick={onClick}
        aria-label={label}
        aria-pressed={active}
        className={cn(
          'focus-ring grid h-8 w-8 place-items-center rounded-lg border shadow-pop backdrop-blur transition-colors',
          active
            ? 'border-emerald-400/40 bg-emerald-400/15 text-emerald-300'
            : 'border-white/[0.09] bg-ink-900/90 text-slate-400 hover:border-white/20 hover:text-white',
        )}
      >
        <Icon className={cn('h-4 w-4', spin && 'animate-spin')} aria-hidden="true" />
      </button>
    </Tooltip>
  );
}

function NodeCard({
  node,
  isContact,
  onClose,
  onCenter,
  onMessage,
  onDetails,
}: {
  node: NodeData;
  isContact: boolean;
  onClose: () => void;
  onCenter: () => void;
  onMessage: () => void;
  onDetails: () => void;
}) {
  return (
    <div className="glass overflow-hidden rounded-xl border border-white/[0.1] shadow-pop animate-slide-up">
      <div className="flex items-start gap-2.5 border-b border-white/[0.06] p-3">
        <Avatar name={node.name} seed={node.node_id} size="md" online={Date.now() - node.last_seen_ms < 10 * 60_000} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <p className="truncate text-[13px] font-semibold text-white">{node.name || 'unnamed node'}</p>
            {isContact ? <Badge tone="accent">contact</Badge> : null}
          </div>
          <p className="mt-0.5 font-mono text-3xs text-slate-500">{shortenNodeId(node.node_id, 6, 4)}</p>
          <div className="mt-1 flex items-center gap-1.5">
            <RoleBadge role={node.class} />
            <span className="text-3xs text-slate-500">{node.hops} hops</span>
          </div>
        </div>
        <IconButton icon={X} label="Close node details" size="sm" onClick={onClose} />
      </div>
      <div className="grid grid-cols-2 gap-x-3 gap-y-2 p-3 text-2xs">
        <Metric icon={Signal} label="RSSI">
          <span className="flex items-center gap-1.5">
            <SignalBars rssi={node.rssi} />
            <span className="font-mono tabular-nums text-slate-300">{node.rssi ?? '—'} dBm</span>
          </span>
        </Metric>
        <Metric icon={BatteryMedium} label="Battery">
          <BatteryIndicator pct={node.battery_pct} />
        </Metric>
        <Metric icon={MapPin} label="Last heard">
          <span className="text-slate-300">{formatLastSeen(node.last_seen_ms)}</span>
        </Metric>
        <Metric icon={ShieldCheck} label="SNR">
          <span className="font-mono text-slate-300">{node.snr ?? '—'} dB</span>
        </Metric>
        <div className="col-span-2 flex items-center justify-between gap-2 border-t border-white/[0.05] pt-2">
          <CapsIcons caps={node.caps} max={8} />
          {node.position ? (
            <span className="font-mono text-3xs text-slate-500">
              {node.position.lat.toFixed(4)}, {node.position.lon.toFixed(4)}
            </span>
          ) : (
            <span className="text-3xs text-slate-600">no position yet</span>
          )}
        </div>
      </div>
      <div className="flex gap-1.5 border-t border-white/[0.06] p-2">
        <Button size="sm" variant="primary" icon={MapPin} className="flex-1" onClick={onCenter} disabled={!node.position}>
          Center
        </Button>
        <Button size="sm" variant="secondary" onClick={onMessage}>
          Message
        </Button>
        <Button size="sm" variant="ghost" onClick={onDetails}>
          Details
        </Button>
      </div>
    </div>
  );
}

function OwnPositionCard({
  position,
  onClose,
  onShare,
  onLocate,
}: {
  position: { lat: number; lon: number; accuracy_m?: number | null; timestamp_ms: number };
  onClose: () => void;
  onShare: () => void;
  onLocate: () => void;
}) {
  return (
    <div className="glass overflow-hidden rounded-xl border border-sky-400/20 shadow-pop animate-slide-up">
      <div className="flex items-center gap-2.5 p-3">
        <span className="grid h-9 w-9 place-items-center rounded-full bg-sky-400/15 text-sky-300">
          <Crosshair className="h-4 w-4" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[13px] font-semibold text-white">Your position</p>
          <p className="font-mono text-3xs text-slate-500">
            {position.lat.toFixed(5)}, {position.lon.toFixed(5)}
          </p>
        </div>
        <IconButton icon={X} label="Close position details" size="sm" onClick={onClose} />
      </div>
      <div className="flex items-center justify-between border-t border-white/[0.06] px-3 py-2 text-3xs text-slate-500">
        <span>±{Math.round(position.accuracy_m ?? 15)} m accuracy</span>
        <span>updated {formatLastSeen(position.timestamp_ms)}</span>
      </div>
      <div className="flex gap-1.5 border-t border-white/[0.06] p-2">
        <Button size="sm" variant="primary" icon={Share2} className="flex-1" onClick={onShare}>
          Share
        </Button>
        <Button size="sm" variant="secondary" icon={LocateFixed} onClick={onLocate}>
          Update
        </Button>
      </div>
    </div>
  );
}

function Metric({
  icon: Icon,
  label,
  children,
}: {
  icon: typeof Signal;
  label: string;
  children: ReactNode;
}) {
  return (
    <div>
      <p className="flex items-center gap-1 text-3xs uppercase tracking-[0.1em] text-slate-600">
        <Icon className="h-2.5 w-2.5" aria-hidden="true" />
        {label}
      </p>
      <div className="mt-0.5">{children}</div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Share position modal                                               */
/* ------------------------------------------------------------------ */

function SharePositionModal({
  open,
  onClose,
  channels,
  contacts,
  onShare,
}: {
  open: boolean;
  onClose: () => void;
  channels: Record<string, { channel_id: string; name: string }>;
  contacts: Record<string, { node_id: string; name: string }>;
  onShare: (target: { channelId?: string; peer?: string }) => void;
}) {
  const [target, setTarget] = useState<string>('');
  const channelList = Object.values(channels);
  const contactList = Object.values(contacts);

  useEffect(() => {
    if (!open) return;
    setTarget(channelList[0] ? `channel:${channelList[0].channel_id}` : contactList[0] ? `peer:${contactList[0].node_id}` : '');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const options = [
    ...channelList.map((channel) => ({ value: `channel:${channel.channel_id}`, label: `# ${channel.name}`, hint: 'channel' })),
    ...contactList.map((contact) => ({ value: `peer:${contact.node_id}`, label: contact.name, hint: 'direct' })),
  ];

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Share my position"
      description="Sends your current coordinates (and accuracy) as a position message on the mesh."
      icon={Share2}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            icon={MapPin}
            disabled={!target}
            onClick={() => {
              const [kind, id] = target.split(':');
              onShare(kind === 'channel' ? { channelId: id } : { peer: id });
            }}
          >
            Share position
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <div className="space-y-1.5">
          <p className="text-2xs font-semibold uppercase tracking-[0.12em] text-slate-500">Send to</p>
          {options.length === 0 ? (
            <p className="text-xs text-slate-500">No channels or contacts yet — create a channel first.</p>
          ) : (
            <div className="space-y-1">
              {options.map((option) => (
                <label
                  key={option.value}
                  className={cn(
                    'flex cursor-pointer items-center gap-2.5 rounded-lg border px-2.5 py-2 text-xs transition-colors',
                    target === option.value
                      ? 'border-emerald-400/40 bg-emerald-400/[0.07] text-slate-100'
                      : 'border-white/[0.06] bg-white/[0.02] text-slate-300 hover:bg-white/[0.04]',
                  )}
                >
                  <input
                    type="radio"
                    name="share-target"
                    value={option.value}
                    checked={target === option.value}
                    onChange={() => setTarget(option.value)}
                    className="h-3 w-3 accent-emerald-400"
                  />
                  <span className="min-w-0 flex-1 truncate">{option.label}</span>
                  <span className="text-3xs text-slate-500">{option.hint}</span>
                </label>
              ))}
            </div>
          )}
        </div>
        <div className="flex items-center gap-2 rounded-lg border border-white/[0.06] bg-white/[0.02] px-2.5 py-2 text-3xs text-slate-500">
          Positions are visible to relays as metadata; content is sealed like any other message.
        </div>
      </div>
    </Modal>
  );
}

export default MapScreen;
