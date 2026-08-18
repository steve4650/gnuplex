import { useEffect, useState } from "react";
import "./Popup.css";
import { API } from "../../lib/API";
import { secondsToTimeComponents, timeComponentsToSeconds } from "../../lib/Helpers";
import { useEscapeKey } from "../../lib/useEscapeKey";

function PosInputPopup(props: {
  visible: boolean;
  setPosInputPopup: React.Dispatch<React.SetStateAction<boolean>>;
  currentPos: number;
  maxPos: number;
  setPos: React.Dispatch<React.SetStateAction<number>>;
}) {
  const [hours, setHours] = useState(0);
  const [minutes, setMinutes] = useState(0);
  const [seconds, setSeconds] = useState(0);

  useEscapeKey(() => {
    props.setPosInputPopup(false);
  }, props.visible);

  useEffect(() => {
    const { hours, minutes, seconds } = secondsToTimeComponents(props.currentPos);
    setHours(hours);
    setMinutes(minutes);
    setSeconds(seconds);
  }, [props.visible]);

  const getTotalSeconds = () => {
    return timeComponentsToSeconds({
      hours: hours,
      minutes: minutes,
      seconds: seconds,
    });
  };

  if (props.visible) {
    return (
      <div className="popup bg-white dark:bg-stone-800 m-5">
        <div className="flex flex-row mb-2 items-center">
          <span className="header mr-1">Position</span>
          <div className="flex gap-1">
            <input
              type="text"
              inputMode="numeric"
              pattern="[0-9]*"
              className="bg-cyan-800 text-slate-100 border border-black text-sm font-mono font-bold p-1 w-16"
              value={String(hours).padStart(2, "0")}
              onChange={(e) => {
                const v = e.target.value.replace(/\D/g, "");
                const n = v === "" ? 0 : parseInt(v, 10);
                setHours(Math.max(0, n));
              }}
              title="Hours"
            />
            <span className="header">:</span>
            <input
              type="text"
              inputMode="numeric"
              pattern="[0-9]*"
              className="bg-cyan-800 text-slate-100 border border-black text-sm font-mono font-bold p-1 w-16"
              value={String(minutes).padStart(2, "0")}
              onChange={(e) => {
                const v = e.target.value.replace(/\D/g, "");
                const n = v === "" ? 0 : parseInt(v, 10);
                setMinutes(Math.min(59, Math.max(0, n)));
              }}
              title="Minutes"
            />
            <span className="header">:</span>
            <input
              type="text"
              inputMode="numeric"
              pattern="[0-9]*"
              className="bg-cyan-800 text-slate-100 border border-black text-sm font-mono font-bold p-1 w-16"
              value={String(seconds).padStart(2, "0")}
              onChange={(e) => {
                const v = e.target.value.replace(/\D/g, "");
                const n = v === "" ? 0 : parseInt(v, 10);
                setSeconds(Math.min(59, Math.max(0, n)));
              }}
              title="Seconds"
            />
          </div>
        </div>
        <div className="flex flex-row">
          <input
            type="button"
            value="OK"
            className="btn-standard mr-1"
            onClick={() => {
              const totalSeconds = getTotalSeconds();
              if (totalSeconds <= props.maxPos) {
                props.setPos(totalSeconds);
                API.setPos(totalSeconds);
                props.setPosInputPopup(false);
              }
            }}
          />
          <input
            type="button"
            value="Cancel"
            className="btn-standard"
            onClick={() => {
              const totalSecs = props.currentPos;
              const { hours, minutes, seconds } = secondsToTimeComponents(totalSecs);
              setHours(hours);
              setMinutes(minutes);
              setSeconds(seconds);
              props.setPosInputPopup(false);
            }}
          />
        </div>
      </div>
    );
  }
  return null;
}

export { PosInputPopup };
