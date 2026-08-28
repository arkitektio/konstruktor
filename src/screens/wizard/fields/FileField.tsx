import { useController } from "react-hook-form";
import { open } from "@tauri-apps/plugin-dialog";

export const FileField = ({ name }: { name: string }) => {
  const { field, fieldState } = useController({ name });

  const chooseFile = async () => {
    const res = await open({
      directory: true,
      title: "Choose an App directory",
    });
    field.onChange(res);
  };

  return (
    <>
      <label>
        {field.value && <div className="font-light my-2">{field.value}</div>}
        <button
          className="border shadow-xl shadow-gray-300/20 rounded border-gray-400 p-1"
          onClick={() => chooseFile()}
        >
          Choose File{" "}
        </button>
      </label>
      {fieldState.error ? (
        <div className="error">{fieldState.error.message}</div>
      ) : null}
    </>
  );
};
