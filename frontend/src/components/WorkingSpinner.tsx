function WorkingSpinnerTSX(props: { visible: boolean }) {
  return (
    <span className="font-mono text-base dark:text-white m-2">{props.visible ? "↺" : " "}</span>
  );
}

export { WorkingSpinnerTSX };
