import type { MediaItem } from "../lib/API";
import { MediaItemButton } from "./Medialist/MediaItemButton";
import { PageSelector } from "./Medialist/PageSelector";

function Medialist(props: {
  mediaItems: (MediaItem | null)[];
  subtitle: string;
  mediaItemCount: number | null;
  paginationOffset: number | null;
  setPaginationOffset: React.Dispatch<React.SetStateAction<number>> | null;
  setQueueingTargetMediaItem: React.Dispatch<React.SetStateAction<MediaItem | null>>;
  setQueueIndex?: React.Dispatch<React.SetStateAction<number | null>>;
}) {
  if (props.mediaItems.length === 0 || props.mediaItems[0] === null) {
    return null;
  }

  return (
    <div className="w-full flex flex-col mb-2 pl-2 whitespace-pre=wrap">
      <div className="flex flex-row align-center mb-1">
        <h1 className="header">{props.subtitle}</h1>
        <PageSelector
          mediaItemCount={props.mediaItemCount}
          paginationOffset={props.paginationOffset}
          setPaginationOffset={props.setPaginationOffset}
        />
      </div>
      {props.mediaItems
        .filter((mediaItem) => mediaItem !== null)
        .map((mediaItem, i: number) => (
          <MediaItemButton
            key={`mediaitem-${mediaItem.Path}`}
            mediaItem={mediaItem}
            setQueueingTargetMediaItem={props.setQueueingTargetMediaItem}
            queueIndex={i}
            setQueueIndex={props.setQueueIndex}
          />
        ))}
    </div>
  );
}

export { Medialist };
