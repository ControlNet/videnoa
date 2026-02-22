import { describe, it, expect } from 'vitest';
import type { Node, Edge } from '@xyflow/react';
import { computeLayout } from '../auto-layout';
import type { PipelineNodeData } from '@/types';

function makeNode(id: string, nodeType: PipelineNodeData['nodeType']): Node<PipelineNodeData> {
  return {
    id,
    type: 'pipeline',
    position: { x: 0, y: 0 },
    data: { nodeType, params: {} },
  };
}

function makeEdge(source: string, target: string): Edge {
  return {
    id: `e-${source}-${target}`,
    source,
    target,
    sourceHandle: 'frames',
    targetHandle: 'frames',
  };
}

describe('computeLayout', () => {
  const nodes: Node<PipelineNodeData>[] = [
    makeNode('input', 'VideoInput'),
    makeNode('sr', 'SuperResolution'),
    makeNode('rife', 'FrameInterpolation'),
    makeNode('output', 'VideoOutput'),
  ];

  const edges: Edge[] = [
    makeEdge('input', 'sr'),
    makeEdge('sr', 'rife'),
    makeEdge('rife', 'output'),
  ];

  it('produces non-overlapping positions for all nodes', () => {
    const positions = computeLayout(nodes, edges);
    const coords = Object.values(positions);

    for (let i = 0; i < coords.length; i++) {
      for (let j = i + 1; j < coords.length; j++) {
        const same = coords[i].x === coords[j].x && coords[i].y === coords[j].y;
        expect(same).toBe(false);
      }
    }
  });

  it('lays out nodes left-to-right (increasing x)', () => {
    const positions = computeLayout(nodes, edges);

    expect(positions['input'].x).toBeLessThan(positions['sr'].x);
    expect(positions['sr'].x).toBeLessThan(positions['rife'].x);
    expect(positions['rife'].x).toBeLessThan(positions['output'].x);
  });

  it('returns positions for all nodes', () => {
    const positions = computeLayout(nodes, edges);
    const ids = Object.keys(positions);

    expect(ids).toHaveLength(4);
    expect(ids).toContain('input');
    expect(ids).toContain('sr');
    expect(ids).toContain('rife');
    expect(ids).toContain('output');
  });
});
