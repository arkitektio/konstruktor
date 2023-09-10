import { Link } from "react-router-dom";
import { useSettings } from "../settings/settings-context";

export const Settings = () => {
  const { settings, setSettings } = useSettings();

  return (
    <div className="h-full w-full relative">
      <div className="text-xl flex flex-row bg-back-800 text-white shadow-xl mb-2 p-2 ">
        <div className="flex-1 my-auto ">
          <Link to="/">{"< Home"}</Link>
        </div>
        <div className="flex-grow my-auto text-center">Settings</div>
      </div>
      <div className="flex flex-col h-full p-2  overflow-y-scroll">
        <div className="border-1 border-gray-300 rounded p-2 bg-white text-black">
          <div className="flex flex-row gap-2 justify-between">
            <div className="flex flex-col gap-2"></div>
          </div>
        </div>
      </div>
    </div>
  );
};
