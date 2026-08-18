import "./Popup.css";
import { API, type MediaItem } from "../../lib/API";
import { useEscapeKey } from "../../lib/useEscapeKey";

function QueuePopup(props: {
  visible: boolean;
  mediaItem: MediaItem | null;
  queueIndex?: number | null;
  setQueueingTargetMediaItem: React.Dispatch<React.SetStateAction<MediaItem | null>>;
  setPos: React.Dispatch<React.SetStateAction<number>>;
  closeHook: () => void;
}) {
  useEscapeKey(() => {
    props.closeHook();
  }, props.visible);
  if (props.visible) {
    return (
      <div className="popup bg-white dark:bg-stone-800 m-5">
        <div className="flex flex-col">
          <input
            type="button"
            value="Play"
            className="btn-standard m-1 min-w-[11ch]"
            onClick={() => {
              if (props.mediaItem) {
                API.playMedia(props.mediaItem, true, false);
              }
              props.closeHook();
            }}
          />

          {props.queueIndex !== null && props.queueIndex !== undefined ? (
            <input
              type="button"
              value="Remove from Queue"
              className="btn-standard m-1 min-w-[11ch]"
              onClick={() => {
                if (
                  props.mediaItem &&
                  props.queueIndex !== null &&
                  props.queueIndex !== undefined
                ) {
                  API.deleteQueueEntry(props.queueIndex);
                  props.setPos(0);
                }
                props.closeHook();
              }}
            />
          ) : null}

          <input
            type="button"
            value="Queue Next"
            className="btn-standard m-1 min-w-[11ch]"
            onClick={() => {
              if (props.mediaItem) {
                API.playMedia(props.mediaItem, true, false);
              }
              props.closeHook();
            }}
          />
          <input
            type="button"
            value="Queue Last"
            className="btn-standard m-1 min-w-[11ch]"
            onClick={() => {
              if (props.mediaItem) {
                API.playMedia(props.mediaItem, false, true);
              }
              props.closeHook();
            }}
          />
          <input
            type="button"
            value="Cancel"
            className="btn-standard m-1 min-w-[11ch]"
            onClick={() => {
              props.closeHook();
            }}
          />
        </div>
      </div>
    );
  }
  return null;
}

export { QueuePopup };
