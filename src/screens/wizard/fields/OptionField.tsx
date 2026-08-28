import { useController } from "react-hook-form";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../../../components/ui/select";

export const OptionField = (props: {
  name: string;
  options: {
    value: string;
    label: string;
  }[];
}) => {
  const { field } = useController({ name: props.name });
  return (
    <Select onValueChange={(v) => field.onChange(v)} value={field.value}>
      <SelectTrigger>
        <SelectValue placeholder="Select.." />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {props.options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
};
