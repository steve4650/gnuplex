import { useEffect, useState } from "react";
import "./Popup.css";
import { API } from "../../lib/API";
import { useEscapeKey } from "../../lib/useEscapeKey";

function VolInputPopup(props: {
  visible: boolean;
  setVolInputPopup: React.Dispatch<React.SetStateAction<boolean>>;
  currentVol: number;
  setVol: React.Dispatch<React.SetStateAction<number>>;
}) {
  const [vol, setVol] = useState(props.currentVol);

  useEscapeKey(() => {
    props.setVolInputPopup(false);
  }, props.visible);

  useEffect(() => {
    setVol(props.currentVol);
  }, [props.visible]);

  if (props.visible) {
    return (
      <div className="popup bg-white dark:bg-stone-800 m-5">
        <div className="flex flex-row mb-2 items-center">
          <span className="header mr-1">Volume</span>
          <input
            type="number"
            className="bg-cyan-800 text-slate-100 border border-black text-sm font-mono font-bold p-1"
            value={vol}
            min={0}
            max={120}
            onChange={(e) => {
              setVol(Math.min(120, Math.max(0, e.target.valueAsNumber || 0)));
            }}
          />
        </div>
        <div className="flex flex-row">
          <input
            type="button"
            value="OK"
            className="btn-standard mr-1"
            onClick={() => {
              props.setVol(vol);
              API.setVol(vol);
              props.setVolInputPopup(false);
            }}
          />
          <input
            type="button"
            value="Cancel"
            className="btn-standard"
            onClick={() => {
              setVol(props.currentVol);
              props.setVolInputPopup(false);
            }}
          />
        </div>
      </div>
    );
  }
  return null;
}

export { VolInputPopup };
