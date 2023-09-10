import { CaretSortIcon, CheckIcon } from "@radix-ui/react-icons";

import { useEffect, useState } from "react";
import { useField } from "formik";
import { Badge } from "../../../components/ui/badge";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "../../../components/ui/popover";
import { Button } from "../../../components/ui/button";
import { cn } from "../../../utils";
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
} from "../../../components/ui/command";

export type Option = {
  label: string;
  value: string;
};

export const ListButtonLabel = (props: {
  value: string[] | undefined;
  setValue: (value: string[]) => void;
  placeholder?: string;
  options: Option[];
}) => {
  const remove = (value: string) => {
    props.setValue(props.value?.filter((v) => v !== value) || []);
  };

  return (
    <div className="gap-2 flex flex-row flex-wrap">
      {props.options
        .filter((l) => props.value?.includes(l.value))
        .map((l, index) => (
          <Badge key={index} onDoubleClick={() => remove(l.value)}>
            {l.label}
          </Badge>
        ))}
      {props.options.length == 0 && (
        <span className="text-muted-foreground">No options selected</span>
      )}
    </div>
  );
};

export type ListSearchFieldProps = {
  name: string;
  label?: string;
  description?: string;
  placeholder?: string;
  options: Option[];
};

export const MultiOptionField = ({
  name,
  label,
  options,
  placeholder = "Please Select",
  description,
}: ListSearchFieldProps) => {
  const [field, meta, helpers] = useField<string[]>(name);

  const [error, setError] = useState<string | null>(null);

  const [searchOptions, setSearchOptions] = useState<Option[]>(options);

  const query = (query: string) => {
    setSearchOptions(
      options.filter((option) =>
        option.label.toLowerCase().includes(query.toLowerCase())
      )
    );
  };

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          role="combobox"
          className={cn(
            "justify-between overflow-hidden truncate ellipsis w-full",
            !field.value && "text-muted-foreground"
          )}
        >
          {field.value ? (
            <ListButtonLabel
              options={options}
              value={field.value}
              placeholder={placeholder}
              setValue={(value) => {
                helpers.setValue(value, true);
              }}
            />
          ) : (
            <>{error ? error : placeholder}</>
          )}
          <CaretSortIcon className="ml-2 h-4 w-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[400px] p-0">
        <Command shouldFilter={false}>
          <CommandInput
            placeholder={placeholder}
            className="h-9"
            onValueChange={(e) => {
              query(e);
            }}
          />
          {searchOptions.length > 0 && (
            <CommandGroup heading="Search">
              {searchOptions.map((option) => (
                <CommandItem
                  value={option.value}
                  key={option.value}
                  onSelect={() => {
                    console.log(option.value);
                    if (field.value == undefined) {
                      field.onChange([option.value]);
                    } else {
                      if (field.value.includes(option.value)) {
                        helpers.setValue(
                          field.value.filter((v: string) => v !== option.value),
                          true
                        );
                      } else {
                        helpers.setValue([...field.value, option.value], true);
                      }
                    }
                  }}
                >
                  {option.label}
                  <CheckIcon
                    className={cn(
                      "ml-auto h-4 w-4",
                      field.value && field.value.includes(option.value)
                        ? "opacity-100"
                        : "opacity-0"
                    )}
                  />
                </CommandItem>
              ))}
            </CommandGroup>
          )}
        </Command>
      </PopoverContent>
    </Popover>
  );
};
