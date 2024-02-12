import * as zod from 'zod'
import { halloPluginSchema } from './HalloPlugin'
import { portPluginSchema } from './PortPlugin'
import { useEffect, useState } from 'react'
import { useFormikContext } from 'formik'





export const hideSchema = zod.object({
    type: zod.literal("hide"),
    key: zod.string(),
    value: zod.optional(zod.any()),
})

export const showSchema = zod.object({
    type: zod.literal("show"),
    key: zod.string(),
    value: zod.optional(zod.any()),
})



export const effectSchema = zod.discriminatedUnion(
    "type",
    [hideSchema, showSchema]
    
)


export type Effect = zod.infer<typeof effectSchema>



export const getNestedValue = (values: any, key: string) => {
    let i = values;


    key.split(".").forEach((key) => {
        i = i[key]
    
    
})

return i

}




export const useEffects = (effects: Effect[] | undefined) => {

    const {values} = useFormikContext()
    const [show, setShow] = useState(true) 


    useEffect(() => {
        
        effects?.forEach(e => {
            switch (e.type) {
                case "hide": {
                    if (getNestedValue(values, e.key) == e.value){
                        setShow(false)
                    }
                }
                case "show": {
                    if (getNestedValue(values, e.key) == e.value){
                        setShow(true)
                    }
                }
                


            }




        })





    }, [values])




    
}


