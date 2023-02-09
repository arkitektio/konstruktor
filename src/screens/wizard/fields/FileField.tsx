import { useField } from "formik";
import { open } from "@tauri-apps/api/dialog";

export const FileField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField(props);

  const chooseFile = async () => {
    const res = await open({
      directory: true,
      title: "Choose an App directory",
    });
    helpers.setValue(res);
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
      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
