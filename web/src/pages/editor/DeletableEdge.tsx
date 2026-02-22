import { BaseEdge, EdgeToolbar, getBezierPath, type EdgeProps } from '@xyflow/react';
import { useWorkflowStore } from '@/stores/workflow-store';
import { X } from 'lucide-react';

export function DeletableEdge(props: EdgeProps) {
  const {
    id,
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    style,
    markerEnd,
  } = props;

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
      <BaseEdge id={id} path={edgePath} style={style} markerEnd={markerEnd} />
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
