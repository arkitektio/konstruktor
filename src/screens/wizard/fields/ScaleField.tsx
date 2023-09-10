import { useField } from "formik";

export type Scale = {
  value: string;
  label: string;
  icon?: string;
  description?: string;
  experimental?: boolean;
};

export const scaleOptions: Scale[] = [
  {
    value: "dev",
    label: "Dev",
    icon: "👩‍🔧",
    description: "Arkitekt is in dev mode (you can't use this in production)",
  },
  { value: "tiny", label: "Tiny", description: "A tiny scale", icon: "🤏" },
  { value: "small", label: "Small", experimental: true },
  { value: "medium", label: "Medium", experimental: true, icon: "🏫" },
  { value: "large", label: "Large", experimental: true, icon: "🏫" },
];

export const ScaleField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<Scale>(props);

  return (
    <>
      <label>
        {field.value && (
          <div className="text-center mt-2">
            <div className="font-light text-5xl">{field.value.icon}</div>
            <div className="font-light">{field.value.label}</div>
            <div className="font-light">{field.value.description}</div>
            <div className="font-light">{field.value.experimental}</div>
          </div>
        )}
        <input
          type="range"
          onChange={(e) =>
            helpers.setValue(scaleOptions[parseInt(e.target.value)])
          }
          value={scaleOptions.findIndex((o) => o.value === field?.value?.value)}
          step={1}
          min={0}
          max={scaleOptions.length - 1}
        />
      </label>
      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
