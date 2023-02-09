import { useState } from "react";
import { Combobox } from "@headlessui/react";

const people = [
  { id: 1, name: "Durward Reynolds" },
  { id: 2, name: "Kenton Towne" },
  { id: 3, name: "Therese Wunsch" },
  { id: 4, name: "Benedict Kessler" },
  { id: 5, name: "Katelyn Rohan" },
];

export type Person = typeof people[number];

export const MultiSelect = () => {
  const [selectedPeople, setSelectedPeople] = useState([people[0], people[1]]);
  const [query, setQuery] = useState("");

  const filteredPeople =
    query === ""
      ? people
      : people.filter((person) => {
          return person.name.toLowerCase().includes(query.toLowerCase());
        });

  return (
    <Combobox<Person[]>
      value={selectedPeople}
      onChange={setSelectedPeople}
      multiple
    >
      <Combobox.Label>Assignee:</Combobox.Label>

      <div className="flex flex-row bg-white text-black focus-within:ring-5 focus-within:ring ring-primary-400 rounded ring-offset-1 gap-1 p-1">
        {selectedPeople.map((person) => (
          <div
            key={person.id}
            className="group flex-shrink flex flex-row border-gray-400 border border-1 rounded-md"
          >
            <div className="p-1 flex-1">{person.name}</div>
            <button
              onClick={() =>
                setSelectedPeople((people) =>
                  people.filter((p) => p.id != person.id)
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
        {filteredPeople.length === 0 && query != "" && (
          <div className="relative cursor-default select-none py-2 px-4 text-gray-700">
            No results found
          </div>
        )}
        {filteredPeople.map((person) => (
          <Combobox.Option
            key={person.id}
            value={person}
            className={({ active }) =>
              `relative cursor-default select-none py-2 pl-10 pr-4 ${
                active ? "bg-primary-400 text-white" : "text-gray-900"
              }`
            }
          >
            {person.name}
          </Combobox.Option>
        ))}
      </Combobox.Options>
    </Combobox>
  );
};
