/*
Copyright 2022.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

package bpfdagent

import (
	"context"
	"fmt"

	"k8s.io/apimachinery/pkg/types"

	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/builder"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/log"
	"sigs.k8s.io/controller-runtime/pkg/predicate"
	"sigs.k8s.io/controller-runtime/pkg/source"

	bpfdiov1alpha1 "github.com/bpfd-dev/bpfd/bpfd-operator/apis/v1alpha1"
	bpfdagentinternal "github.com/bpfd-dev/bpfd/bpfd-operator/controllers/bpfd-agent/internal"
	"github.com/bpfd-dev/bpfd/bpfd-operator/internal"

	gobpfd "github.com/bpfd-dev/bpfd/clients/gobpfd/v1"
	v1 "k8s.io/api/core/v1"
)

//+kubebuilder:rbac:groups=bpfd.dev,resources=tcprograms,verbs=get;list;watch

// TcProgramReconciler reconciles a tcProgram object by creating multiple
// bpfProgram objects and managing bpfd for each one.
type TcProgramReconciler struct {
	ReconcilerCommon
	currentTcProgram *bpfdiov1alpha1.TcProgram
	ourNode          *v1.Node
	interfaces       []string
}

func (r *TcProgramReconciler) getRecCommon() *ReconcilerCommon {
	return &r.ReconcilerCommon
}

func (r *TcProgramReconciler) getFinalizer() string {
	return internal.TcProgramControllerFinalizer
}

func (r *TcProgramReconciler) getRecType() string {
	return internal.Tc.String()
}

// Must match with bpfd internal types
func tcProceedOnToInt(proceedOn []bpfdiov1alpha1.TcProceedOnValue) []int32 {
	var out []int32

	for _, p := range proceedOn {
		switch p {
		case "unspec":
			out = append(out, -1)
		case "ok":
			out = append(out, 0)
		case "reclassify":
			out = append(out, 1)
		case "shot":
			out = append(out, 2)
		case "pipe":
			out = append(out, 3)
		case "stolen":
			out = append(out, 4)
		case "queued":
			out = append(out, 5)
		case "repeat":
			out = append(out, 6)
		case "redirect":
			out = append(out, 7)
		case "trap":
			out = append(out, 8)
		case "dispatcher_return":
			out = append(out, 30)
		}
	}

	return out
}

// SetupWithManager sets up the controller with the Manager.
// The Bpfd-Agent should reconcile whenever a TcProgram is updated,
// load the program to the node via bpfd, and then create bpfProgram object(s)
// to reflect per node state information.
func (r *TcProgramReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&bpfdiov1alpha1.TcProgram{}, builder.WithPredicates(predicate.And(
			predicate.GenerationChangedPredicate{},
			predicate.ResourceVersionChangedPredicate{}),
		),
		).
		Owns(&bpfdiov1alpha1.BpfProgram{},
			builder.WithPredicates(predicate.And(
				internal.BpfProgramTypePredicate(internal.Tc.String()),
				internal.BpfProgramNodePredicate(r.NodeName)),
			),
		).
		// Only trigger reconciliation if node labels change since that could
		// make the TcProgram no longer select the Node. Additionally only
		// care about events specific to our node
		Watches(
			&source.Kind{Type: &v1.Node{}},
			&handler.EnqueueRequestForObject{},
			builder.WithPredicates(predicate.And(predicate.LabelChangedPredicate{}, nodePredicate(r.NodeName))),
		).
		Complete(r)
}

func (r *TcProgramReconciler) expectedBpfPrograms(ctx context.Context) (*bpfdiov1alpha1.BpfProgramList, error) {
	progs := &bpfdiov1alpha1.BpfProgramList{}
	for _, iface := range r.interfaces {
		bpfProgramName := fmt.Sprintf("%s-%s-%s", r.currentTcProgram.Name, r.NodeName, iface)
		annotations := map[string]string{internal.TcProgramInterface: iface}

		prog, err := r.createBpfProgram(ctx, bpfProgramName, r.getFinalizer(), r.currentTcProgram, r.getRecType(), annotations)
		if err != nil {
			return nil, fmt.Errorf("failed to create BpfProgram %s: %v", bpfProgramName, err)
		}

		progs.Items = append(progs.Items, *prog)
	}

	return progs, nil
}

func (r *TcProgramReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	// Initialize node and current program
	r.currentTcProgram = &bpfdiov1alpha1.TcProgram{}
	r.ourNode = &v1.Node{}
	r.Logger = ctrl.Log.WithName("tc")
	var err error

	ctxLogger := log.FromContext(ctx)
	ctxLogger.Info("Reconcile TC: Enter", "ReconcileKey", req)

	// Lookup K8s node object for this bpfd-agent This should always succeed
	if err := r.Get(ctx, types.NamespacedName{Namespace: v1.NamespaceAll, Name: r.NodeName}, r.ourNode); err != nil {
		return ctrl.Result{Requeue: false}, fmt.Errorf("failed getting bpfd-agent node %s : %v",
			req.NamespacedName, err)
	}

	tcPrograms := &bpfdiov1alpha1.TcProgramList{}

	opts := []client.ListOption{}

	if err := r.List(ctx, tcPrograms, opts...); err != nil {
		return ctrl.Result{Requeue: false}, fmt.Errorf("failed getting TcPrograms for full reconcile %s : %v",
			req.NamespacedName, err)
	}

	if len(tcPrograms.Items) == 0 {
		r.Logger.Info("TcProgramController found no TC Programs")
		return ctrl.Result{Requeue: false}, nil
	}

	// Get existing ebpf state from bpfd.
	existingPrograms, err := bpfdagentinternal.ListBpfdPrograms(ctx, r.BpfdClient, internal.Tc)
	if err != nil {
		r.Logger.Error(err, "failed to list loaded bpfd programs")
		return ctrl.Result{Requeue: true, RequeueAfter: retryDurationAgent}, nil
	}

	// Reconcile each TcProgram. Don't return error here because it will trigger an infinite reconcile loop, instead
	// report the error to user and retry if specified. For some errors the controller may not decide to retry.
	// Note: This only results in grpc calls to bpfd if we need to change something
	requeue := false // initialize requeue to false
	for _, tcProgram := range tcPrograms.Items {
		r.Logger.Info("TcProgramController is reconciling", "currentTcProgram", tcProgram.Name)
		r.currentTcProgram = &tcProgram

		r.interfaces, err = getInterfaces(&r.currentTcProgram.Spec.InterfaceSelector, r.ourNode)
		if err != nil {
			r.Logger.Error(err, "failed to get interfaces for TcProgram")
			return ctrl.Result{Requeue: true, RequeueAfter: retryDurationAgent}, nil
		}

		result, err := reconcileProgram(ctx, r, r.currentTcProgram, &r.currentTcProgram.Spec.BpfProgramCommon, r.ourNode, existingPrograms)
		if err != nil {
			r.Logger.Error(err, "Reconciling TcProgram Failed", "TcProgramName", r.currentTcProgram.Name, "ReconcileResult", result.String())
		}

		switch result {
		case internal.Unchanged:
			// continue with next program
		case internal.Updated:
			// return
			return ctrl.Result{Requeue: false}, nil
		case internal.Requeue:
			// remember to do a requeue when we're done and continue with next program
			requeue = true
		}
	}

	if requeue {
		// A requeue has been requested
		return ctrl.Result{RequeueAfter: retryDurationAgent}, nil
	} else {
		// We've made it through all the programs in the list without anything being
		// updated and a reque has not been requested.
		return ctrl.Result{Requeue: false}, nil
	}
}

func (r *TcProgramReconciler) buildTcLoadRequest(
	bytecode *gobpfd.BytecodeLocation,
	uuid string,
	iface string,
	mapOwnerId *uint32) *gobpfd.LoadRequest {

	return &gobpfd.LoadRequest{
		Bytecode:    bytecode,
		Name:        r.currentTcProgram.Spec.BpfFunctionName,
		ProgramType: uint32(internal.Tc),
		Attach: &gobpfd.AttachInfo{
			Info: &gobpfd.AttachInfo_TcAttachInfo{
				TcAttachInfo: &gobpfd.TCAttachInfo{
					Priority:  r.currentTcProgram.Spec.Priority,
					Iface:     iface,
					Direction: r.currentTcProgram.Spec.Direction,
					ProceedOn: tcProceedOnToInt(r.currentTcProgram.Spec.ProceedOn),
				},
			},
		},
		Metadata:   map[string]string{internal.UuidMetadataKey: uuid, internal.ProgramNameKey: r.currentTcProgram.Name},
		GlobalData: r.currentTcProgram.Spec.GlobalData,
		MapOwnerId: mapOwnerId,
	}
}

// reconcileBpfdPrograms ONLY reconciles the bpfd state for a single BpfProgram.
// It does not interact with the k8s API in any way.
func (r *TcProgramReconciler) reconcileBpfdProgram(ctx context.Context,
	existingBpfPrograms map[string]*gobpfd.ListResponse_ListResult,
	bytecodeSelector *bpfdiov1alpha1.BytecodeSelector,
	bpfProgram *bpfdiov1alpha1.BpfProgram,
	isNodeSelected bool,
	isBeingDeleted bool,
	mapOwnerStatus *MapOwnerParamStatus) (bpfdiov1alpha1.BpfProgramConditionType, error) {

	r.Logger.V(1).Info("Existing bpfProgram", "ExistingMaps", bpfProgram.Spec.Maps, "UUID", bpfProgram.UID, "Name", bpfProgram.Name)
	iface := bpfProgram.Annotations[internal.TcProgramInterface]

	var err error
	uuid := string(bpfProgram.UID)

	getLoadRequest := func() (*gobpfd.LoadRequest, bpfdiov1alpha1.BpfProgramConditionType, error) {
		bytecode, err := bpfdagentinternal.GetBytecode(r.Client, bytecodeSelector)
		if err != nil {
			return nil, bpfdiov1alpha1.BpfProgCondBytecodeSelectorError, fmt.Errorf("failed to process bytecode selector: %v", err)
		}
		loadRequest := r.buildTcLoadRequest(bytecode, string(uuid), iface, mapOwnerStatus.mapOwnerId)
		return loadRequest, bpfdiov1alpha1.BpfProgCondNone, nil
	}

	existingProgram, doesProgramExist := existingBpfPrograms[string(uuid)]
	if !doesProgramExist {
		r.Logger.V(1).Info("TcProgram doesn't exist on node for iface", "interface", iface)

		// If TcProgram is being deleted just break out and remove finalizer
		if isBeingDeleted {
			return bpfdiov1alpha1.BpfProgCondUnloaded, nil
		}

		// Make sure if we're not selected just exit
		if !isNodeSelected {
			return bpfdiov1alpha1.BpfProgCondNotSelected, nil
		}

		// Make sure if the Map Owner is set but not found then just exit
		if mapOwnerStatus.isSet && !mapOwnerStatus.isFound {
			return bpfdiov1alpha1.BpfProgCondMapOwnerNotFound, nil
		}

		// Make sure if the Map Owner is set but not loaded then just exit
		if mapOwnerStatus.isSet && !mapOwnerStatus.isLoaded {
			return bpfdiov1alpha1.BpfProgCondMapOwnerNotLoaded, nil
		}

		// otherwise load it
		loadRequest, condition, err := getLoadRequest()
		if err != nil {
			return condition, err
		}

		r.progId, err = bpfdagentinternal.LoadBpfdProgram(ctx, r.BpfdClient, loadRequest)
		if err != nil {
			r.Logger.Error(err, "Failed to load TcProgram")
			return bpfdiov1alpha1.BpfProgCondNotLoaded, nil
		}

		r.Logger.Info("bpfd called to load TcProgram on Node", "Name", bpfProgram.Name, "UUID", uuid)
		return bpfdiov1alpha1.BpfProgCondLoaded, nil
	}

	// prog ID should already have been set
	id, err := bpfdagentinternal.GetID(bpfProgram)
	if err != nil {
		r.Logger.Error(err, "Failed to get program ID")
		return bpfdiov1alpha1.BpfProgCondNotLoaded, nil
	}

	// BpfProgram exists but either BpfProgramConfig is being deleted or node is no
	// longer selected....unload program
	// BpfProgram exists but either TcProgram is being deleted, node is no
	// longer selected, or map is not available....unload program
	if isBeingDeleted || !isNodeSelected ||
		(mapOwnerStatus.isSet && (!mapOwnerStatus.isFound || !mapOwnerStatus.isLoaded)) {
		r.Logger.V(1).Info("TcProgram exists on Node but is scheduled for deletion, not selected, or map not available",
			"isDeleted", isBeingDeleted, "isSelected", isNodeSelected, "mapIsSet", mapOwnerStatus.isSet,
			"mapIsFound", mapOwnerStatus.isFound, "mapIsLoaded", mapOwnerStatus.isLoaded)

		if err := bpfdagentinternal.UnloadBpfdProgram(ctx, r.BpfdClient, *id); err != nil {
			r.Logger.Error(err, "Failed to unload TcProgram")
			return bpfdiov1alpha1.BpfProgCondNotUnloaded, nil
		}

		r.Logger.Info("bpfd called to unload TcProgram on Node", "Name", bpfProgram.Name, "UUID", id)

		if isBeingDeleted {
			return bpfdiov1alpha1.BpfProgCondUnloaded, nil
		}

		if !isNodeSelected {
			return bpfdiov1alpha1.BpfProgCondNotSelected, nil
		}

		if mapOwnerStatus.isSet && !mapOwnerStatus.isFound {
			return bpfdiov1alpha1.BpfProgCondMapOwnerNotFound, nil
		}

		if mapOwnerStatus.isSet && !mapOwnerStatus.isLoaded {
			return bpfdiov1alpha1.BpfProgCondMapOwnerNotLoaded, nil
		}
	}

	// BpfProgram exists but is not correct state, unload and recreate
	loadRequest, condition, err := getLoadRequest()
	if err != nil {
		return condition, err
	}

	isSame, reasons := bpfdagentinternal.DoesProgExist(existingProgram, loadRequest)
	if !isSame {
		r.Logger.V(1).Info("TcProgram is in wrong state, unloading and reloading", "Reason", reasons)

		if err := bpfdagentinternal.UnloadBpfdProgram(ctx, r.BpfdClient, *id); err != nil {
			r.Logger.Error(err, "Failed to unload TcProgram")
			return bpfdiov1alpha1.BpfProgCondNotUnloaded, nil
		}

		r.progId, err = bpfdagentinternal.LoadBpfdProgram(ctx, r.BpfdClient, loadRequest)
		if err != nil {
			r.Logger.Error(err, "Failed to load TcProgram")
			return bpfdiov1alpha1.BpfProgCondNotLoaded, nil
		}

		r.Logger.Info("bpfd called to reload TcProgram on Node", "Name", bpfProgram.Name, "UUID", id)
	} else {
		// Program exists and bpfProgram K8s Object is up to date
		r.Logger.V(1).Info("Ignoring Object Change nothing to do in bpfd")
		r.progId = id
	}

	return bpfdiov1alpha1.BpfProgCondLoaded, nil
}
