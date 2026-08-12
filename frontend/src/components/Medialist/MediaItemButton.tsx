import { API, type MediaItem } from "../../lib/API";
import { useLongPress } from "../../lib/useLongPress";

function MediaItemButton(props: {
  mediaItem: MediaItem;
  queueIndex?: number;
  setQueueingTargetMediaItem: React.Dispatch<React.SetStateAction<MediaItem | null>>;
  setQueueIndex?: React.Dispatch<React.SetStateAction<number | null>>;
}) {
  const longPressHandlers = useLongPress({
    onShortClick: () => {
      API.playMedia(props.mediaItem, true, false);
    },
    onLongPress: () => {
      props.setQueueingTargetMediaItem(props.mediaItem);
      if (props.setQueueIndex !== undefined && props.queueIndex !== undefined) {
        props.setQueueIndex(props.queueIndex);
      }
    },
  });

  return (
    <input
      type="button"
      className="inline-block w-screen max-w-screen-md text-left hover:bg-cyan-200 hover:dark:bg-cyan-700 p-1 border-lightgray dark:border-stone-800 border-t-2 whitespace-break-spaces break-words dark:bg-stone-950 dark:text-slate-200"
      value={props.mediaItem.Path}
      {...longPressHandlers}
    />
  );
}

export { MediaItemButton };
