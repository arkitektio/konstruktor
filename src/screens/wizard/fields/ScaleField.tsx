import { useField } from "formik";
import { open } from "@tauri-apps/api/dialog";

export const ScaleField = ({ ...props }: any) => {
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
        <input type="range" />
      </label>
      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
