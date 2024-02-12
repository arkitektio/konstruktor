import * as zod from 'zod'
import { halloPluginSchema } from './HalloPlugin'
import { portPluginSchema } from './PortPlugin'




export const pluginSchema = zod.discriminatedUnion(
    "type",
    [halloPluginSchema, portPluginSchema]
    
)

