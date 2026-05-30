function PageSelector(props: {
  mediaItemCount: number | null;
  paginationOffset: number | null;
  setPaginationOffset: React.Dispatch<React.SetStateAction<number>> | null;
}) {
  if (
    props.mediaItemCount === null ||
    props.paginationOffset === null ||
    props.setPaginationOffset === null
  ) {
    return null;
  }
  return props.mediaItemCount < 1000 ? null : (
    <select
      className="select select-sm select-bordered ml-2"
      value={props.paginationOffset}
      onChange={(e) => {
        if (props.setPaginationOffset) {
          props.setPaginationOffset((Number(e.target.value) || 0) * 1000);
        }
      }}
    >
      {[...Array.from({ length: Math.ceil(props.mediaItemCount / 1000) }).keys()].map((i) => (
        <option key={`range-${i}`} value={i}>
          {`${i * 1000}-${Math.min(props.mediaItemCount || 0, (i + 1) * 1000 - 1)}`}
        </option>
      ))}
    </select>
  );
}

export { PageSelector };
