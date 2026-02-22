import { BaseEdge, EdgeToolbar, getBezierPath, type EdgeProps } from '@xyflow/react';
import { useWorkflowStore } from '@/stores/workflow-store';
import { X } from 'lucide-react';

export function TensorEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
}: EdgeProps) {
  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  });

  return (
    <>
      {/* Wide aura for the "highway" footprint */}
      <BaseEdge
        path={edgePath}
        style={{
          stroke: 'url(#tensor-edge-aura)',
          strokeWidth: 12,
          opacity: 0.24,
          filter: 'blur(7px)',
          animation: 'tensor-aura-pulse 1.5s ease-in-out infinite',
        }}
      />

      {/* Main carriageway */}
      <BaseEdge
        path={edgePath}
        style={{
          stroke: 'url(#tensor-edge-base)',
          strokeWidth: 7,
          strokeLinecap: 'round',
          opacity: 0.9,
          animation: 'tensor-core-pulse 0.95s ease-in-out infinite',
        }}
      />

      {/* Fast lane streaks */}
      <BaseEdge
        path={edgePath}
        style={{
          stroke: 'url(#tensor-edge-lane)',
          strokeWidth: 3.5,
          strokeLinecap: 'round',
          strokeDasharray: '20 10',
          animation: 'tensor-lane-rush 0.45s linear infinite',
        }}
      />

      <EdgeToolbar edgeId={id} x={labelX} y={labelY}>
        <button
          type="button"
          className="flex items-center justify-center size-5 rounded-full bg-destructive/80 text-white hover:bg-destructive transition-colors shadow-md"
          onClick={(e) => {
            e.stopPropagation();
            useWorkflowStore.getState().removeEdge(id);
          }}
        >
          <X className="size-3" />
        </button>
      </EdgeToolbar>
    </>
  );
}
