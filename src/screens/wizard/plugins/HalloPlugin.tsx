import * as zod from 'zod'
import { buildPlugin } from './build'


export const halloPluginSchema = zod.object({
    type: zod.literal("hallo"),
    hello: zod.string()
})


type Values = zod.TypeOf<typeof halloPluginSchema>


export const validationSchema = zod.object({})



export const HalloPlugin = (props: {schema: Values}) => {




    return <>
        {props.schema.hello}

        </>






}




export const halloPlugin = buildPlugin(validationSchema, HalloPlugin)






