import React, { useState } from "react";
import { Combobox } from "@headlessui/react";
import { useField } from "formik";

const people = [
  { id: 1, name: "Durward Reynolds" },
  { id: 2, name: "Kenton Towne" },
  { id: 3, name: "Therese Wunsch" },
  { id: 4, name: "Benedict Kessler" },
  { id: 5, name: "Katelyn Rohan" },
];

export type Person = typeof people[number];

export type Option = {
  [key: string]: any;
};

export type Props<T extends Option> = {
  options: T[];
  name: string;
  label: keyof T;
  unique: keyof T;
  description?: keyof T;
};

export const MultiSelectField = <T extends Option>({
  options,
  name,
  label,
  unique,
}: Props<T>) => {
  const [field, meta, helpers] = useField<T[]>(name);
  const [query, setQuery] = useState("");

  const filteredOptions =
    query === ""
      ? options
      : options.filter((person) => {
          return person[label].toLowerCase().includes(query.toLowerCase());
        });

  const setValues = async (values: T[]) => {
    helpers.setValue(values);
    setQuery("");
  };

  return (
    <Combobox<T[]> value={field.value} onChange={setValues} multiple>
      <div className="flex flex-row bg-white text-black focus-within:ring-5 focus-within:ring ring-primary-400 rounded ring-offset-1 gap-1 p-1">
        {field.value.map((option) => (
          <div
            key={option[unique]}
            className="group flex-shrink flex flex-row border-gray-400 border border-1 rounded-md"
          >
            <div className="p-1 flex-1">{option[label]}</div>
            <button
              onClick={() =>
                setValues(
                  field.value.filter((p) => p[unique] != option[unique])
                )
              }
              className="flex-shrink flex flex-row bg-primary-400 hover:bg-primary-600 group-hover:opacity-100 overflow-hidden rounded-r-md"
            >
              <div className="p-1">x</div>
            </button>
          </div>
        ))}
        <Combobox.Input
          onChange={(event) => setQuery(event.target.value)}
          className="flex-grow focus:outline-none"
        />
      </div>
      <Combobox.Options
        className={"bg-white text-black p-1 rounded rounded-md"}
      >
        {filteredOptions.length === 0 && query != "" && (
          <div className="relative cursor-default select-none py-2 px-4 text-gray-700">
            No results found
          </div>
        )}
        {filteredOptions.map((option) => (
          <Combobox.Option
            key={option[unique]}
            value={option}
            className={({ active }) =>
              `relative cursor-default select-none py-2 pl-10 pr-4 ${
                active ? "bg-primary-400 text-white" : "text-gray-900"
              }`
            }
          >
            {option[label]}
          </Combobox.Option>
        ))}
      </Combobox.Options>
    </Combobox>
  );
};

export const MultiSelectReduceField = <T extends Option>({
  options,
  name,
  label,
  unique,
}: Props<T>) => {
  const [field, meta, helpers] = useField<typeof unique[]>(name);
  const [query, setQuery] = useState("");

  const filteredOptions =
    query === ""
      ? options
      : options.filter((person) => {
          return person[label].toLowerCase().includes(query.toLowerCase());
        });

  const setValues = async (values: typeof unique[]) => {
    console.log(values);
    helpers.setValue(values);
  };

  return (
    <Combobox<typeof unique[]>
      value={field.value}
      onChange={setValues}
      multiple
    >
      <div className="flex flex-row bg-white text-black focus-within:ring-5 focus-within:ring ring-primary-400 rounded ring-offset-1 gap-1 p-1">
        {field.value
          .flatMap((v) => options.find((o) => o[unique] == v))
          .map((option) => (
            <div
              key={option[unique]}
              className="group flex-shrink flex flex-row border-gray-400 border border-1 rounded-md"
            >
              <div className="p-1 flex-1">{option[label]}</div>
              <button
                onClick={() =>
                  setValues(field.value.filter((p) => p != option[unique]))
                }
                className="flex-shrink flex flex-row bg-primary-400 hover:bg-primary-600 group-hover:opacity-100 overflow-hidden rounded-r-md"
              >
                <div className="p-1">x</div>
              </button>
            </div>
          ))}
        <Combobox.Input
          onChange={(event) => setQuery(event.target.value)}
          className="flex-grow focus:outline-none"
        />
      </div>
      <Combobox.Options
        className={"bg-white text-black p-1 rounded rounded-md"}
      >
        {filteredOptions.length === 0 && query != "" && (
          <div className="relative cursor-default select-none py-2 px-4 text-gray-700">
            No results found
          </div>
        )}
        {filteredOptions.map((option) => (
          <Combobox.Option
            key={option[unique]}
            value={option[unique]}
            className={({ active }) =>
              `relative cursor-default select-none py-2 pl-10 pr-4 ${
                active ? "bg-primary-400 text-white" : "text-gray-900"
              }`
            }
          >
            {option[label]}
          </Combobox.Option>
        ))}
      </Combobox.Options>
    </Combobox>
  );
};
