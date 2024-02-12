import { ConditionalStep } from "../ChannelSetup";


export type WizardPlugin = {
    component: (props: any) => React.ReactNode
    validationSchema: Zod.Schema
}


export const buildPlugin = (validationSchema: Zod.Schema, component:(props: any) => React.ReactNode): WizardPlugin => {
    return {
        
        component,
        validationSchema
    }



}