import { useField } from "formik";
import { useChannel } from "../../../channel/channel-context";
import { Hover } from "../../../layout/Hover";
import { Service } from "../../../repo/repo-context";

const isRequiredBy = (
  name: string,
  service: Service,
  availableServices: Service[]
) => {
  return availableServices
    .find((s) => s.name === name)
    ?.requires?.some((r) => r === service.interface);
};

export const ServiceSelectionField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<string[]>(props);
  const { availableServices } = useChannel();

  const toggleValue = async (service: Service) => {
    if (field.value) {
      if (field.value.find((i: string) => i === service.name)) {
        helpers.setValue(
          field.value.filter(
            (i) =>
              i !== service.name &&
              isRequiredBy(i, service, availableServices || []) === false
          )
        );
      } else {
        helpers.setValue([
          ...field.value,
          service.name,
          ...(availableServices || [])
            .filter((s) => service.requires?.includes(s.interface))
            .map((s) => s.name),
        ]);
      }
    } else {
      helpers.setValue([service.name]);
    }
    console.log(field.value);
  };

  return (
    <>
      <Hover className="grid grid-cols-4 flex-wrap gap-2 p-2">
        {availableServices?.map((service, i) => (
          <div
            className={`hovercard cursor-pointer border border-1 flex flex-col bg-back-800 p-3 ${
              field.value && field.value.find((i) => i === service.name)
                ? " border-slate-200 "
                : "shadow-primary-300/20 border-slate-400 opacity-40 "
            }`}
            key={i}
            onClick={() => toggleValue(service)}
          >
            <div className="flex flex-row justify-between">
              {service.logo && (
                <img className="text-sm text-start h-20" src={service.logo} />
              )}
            </div>
            <div className="font-bold text-start flex-row flex justify-between">
              <div className="my-auto">{service.name}</div>
              {service.experimental && (
                <div className="text-xs border-red-300 border p-1 rounded rounded-md">
                  Exp
                </div>
              )}
            </div>
            <div className="font-light  text-start">{service.description}</div>
            <div className="text-sm  text-start mt-1">{service.long}</div>
          </div>
        ))}
      </Hover>

      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
