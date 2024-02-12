import * as zod from 'zod'
import { buildPlugin } from './build'



export const portPluginSchema = zod.object({
    type: zod.literal("port"),
    ports: zod.array(zod.object({
            name: zod.string()
    }))
})


export const validationSchema = zod.object({})

type Values = zod.TypeOf<typeof portPluginSchema>


export const PortPlugin = (props: {schema: Values}) => {




    return <>
        {props.schema.ports?.map(port => <>
            {port.name}
            </> )}


        </>






}

export const portPlugin = buildPlugin(validationSchema, PortPlugin)