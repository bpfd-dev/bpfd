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

package bpfdoperator

import (
	"context"
	"fmt"
	"reflect"
	"time"

	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	"sigs.k8s.io/controller-runtime/pkg/event"
	"sigs.k8s.io/controller-runtime/pkg/predicate"

	bpfdiov1alpha1 "github.com/bpfd-dev/bpfd/bpfd-operator/apis/v1alpha1"
	bpfdagent "github.com/bpfd-dev/bpfd/bpfd-operator/controllers/bpfd-agent"
	"github.com/go-logr/logr"
)

//+kubebuilder:rbac:groups=bpfd.io,resources=bpfprograms,verbs=get;list;watch
//+kubebuilder:rbac:groups=core,resources=nodes,verbs=get;list;watch

type BpfProgramConfigConditionType string

const (
	bpfdOperatorFinalizer                                       = "bpfd.io.operator/finalizer"
	retryDurationOperator                                       = 5 * time.Second
	BpfProgConfigNotYetLoaded     BpfProgramConfigConditionType = "NotYetLoaded"
	BpfProgConfigReconcileError   BpfProgramConfigConditionType = "ReconcileError"
	BpfProgConfigReconcileSuccess BpfProgramConfigConditionType = "ReconcileSuccess"
	BpfProgConfigDeleteError      BpfProgramConfigConditionType = "DeleteError"
)

func (b BpfProgramConfigConditionType) Condition(message string) metav1.Condition {
	cond := metav1.Condition{}

	switch b {
	case BpfProgConfigNotYetLoaded:
		if len(message) == 0 {
			message = "Waiting for BpfProgramConfig Object to be reconciled to all nodes"
		}

		cond = metav1.Condition{
			Type:    string(BpfProgConfigNotYetLoaded),
			Status:  metav1.ConditionTrue,
			Reason:  "ProgramsNotYetLoaded",
			Message: message,
		}
	case BpfProgConfigReconcileError:
		if len(message) == 0 {
			message = "bpfProgramReconciliation failed"
		}

		cond = metav1.Condition{
			Type:    string(BpfProgConfigReconcileError),
			Status:  metav1.ConditionTrue,
			Reason:  "ReconcileError",
			Message: message,
		}
	case BpfProgConfigReconcileSuccess:
		if len(message) == 0 {
			message = "bpfProgramReconciliation Succeeded on all nodes"
		}

		cond = metav1.Condition{
			Type:    string(BpfProgConfigReconcileSuccess),
			Status:  metav1.ConditionTrue,
			Reason:  "ReconcileSuccess",
			Message: message,
		}
	case BpfProgConfigDeleteError:
		if len(message) == 0 {
			message = "bpfProgramConfig Deletion failed"
		}

		cond = metav1.Condition{
			Type:    string(BpfProgConfigDeleteError),
			Status:  metav1.ConditionTrue,
			Reason:  "DeleteError",
			Message: message,
		}
	}

	return cond
}

// ReconcilerCommon reconciles a BpfProgram object
type ReconcilerCommon struct {
	client.Client
	Scheme *runtime.Scheme
	Logger logr.Logger
}

// bpfdReconciler defines a k8s reconciler which can program bpfd.
type ProgramReconciler interface {
	getRecCommon() *ReconcilerCommon
	updateStatus(ctx context.Context,
		name string,
		cond BpfProgramConfigConditionType,
		message string) (ctrl.Result, error)
	getFinalizer() string
}

func reconcileBpfProgram(ctx context.Context, rec ProgramReconciler, prog client.Object) (ctrl.Result, error) {
	r := rec.getRecCommon()
	progName := prog.GetName()

	r.Logger.V(1).Info("Reconciling bpfProgramConfig", "bpfProgramConfig", progName)

	if !controllerutil.ContainsFinalizer(prog, bpfdOperatorFinalizer) {
		return r.addFinalizer(ctx, prog, bpfdOperatorFinalizer)
	}

	// reconcile BpfProgramConfig Object on all other events
	// list all existing bpfProgram state for the given BpfProgramConfig
	bpfPrograms := &bpfdiov1alpha1.BpfProgramList{}

	// Only list bpfPrograms for this BpfProgramConfig
	opts := []client.ListOption{client.MatchingLabels{"owningConfig": progName}}

	if err := r.List(ctx, bpfPrograms, opts...); err != nil {
		r.Logger.Error(err, "failed to get freshBpfProgramConfigs for full reconcile")
		return ctrl.Result{}, nil
	}

	// List all nodes since an bpfprogram object will always be created for each
	nodes := &corev1.NodeList{}
	if err := r.List(ctx, nodes, &client.ListOptions{}); err != nil {
		r.Logger.Error(err, "failed getting nodes for full reconcile")
		return ctrl.Result{Requeue: true, RequeueAfter: retryDurationOperator}, nil
	}

	// Return NotYetLoaded Status if
	// BpfPrograms for each node haven't been created by bpfd-agent and the config isn't
	// being deleted.
	if len(nodes.Items) != len(bpfPrograms.Items) && prog.GetDeletionTimestamp().IsZero() {
		// Causes Requeue
		return rec.updateStatus(ctx, progName, BpfProgConfigNotYetLoaded, "")
	}

	failedBpfPrograms := []string{}
	finalApplied := []string{}
	// Make sure no bpfPrograms had any issues in the loading or unloading process
	for _, bpfProgram := range bpfPrograms.Items {

		if controllerutil.ContainsFinalizer(&bpfProgram, rec.getFinalizer()) {
			finalApplied = append(finalApplied, bpfProgram.Name)
		}

		if bpfProgram.Status.Conditions == nil {
			break
		}

		// Get most recent condition
		recentIdx := len(bpfProgram.Status.Conditions) - 1

		condition := bpfProgram.Status.Conditions[recentIdx]

		if condition.Type == string(bpfdagent.BpfProgCondNotLoaded) || condition.Type == string(bpfdagent.BpfProgCondNotUnloaded) {
			failedBpfPrograms = append(failedBpfPrograms, bpfProgram.Name)
		}
	}

	if !prog.GetDeletionTimestamp().IsZero() {
		// Only remove bpfd-operator finalizer if all bpfProgram Objects are ready to be pruned  (i.e finalizers have been removed)
		if len(finalApplied) == 0 {
			// Causes Requeue
			return r.removeFinalizer(ctx, prog, bpfdOperatorFinalizer)
		}

		// Causes Requeue
		return rec.updateStatus(ctx, progName, BpfProgConfigDeleteError, fmt.Sprintf("bpfProgramConfig Deletion failed on the following bpfProgram Objects: %v",
			finalApplied))
	}

	if len(failedBpfPrograms) != 0 {
		// Causes Requeue
		return rec.updateStatus(ctx, progName, BpfProgConfigReconcileError,
			fmt.Sprintf("bpfProgramReconciliation failed on the following bpfProgram Objects: %v", failedBpfPrograms))
	}

	// Causes Requeue
	return rec.updateStatus(ctx, progName, BpfProgConfigReconcileSuccess, "")
}

func (r *ReconcilerCommon) removeFinalizer(ctx context.Context, prog client.Object, finalizer string) (ctrl.Result, error) {
	r.Logger.V(1).Info("Program is deleted remove finalizer", "ProgramName", prog.GetName())

	if changed := controllerutil.RemoveFinalizer(prog, finalizer); changed {
		err := r.Update(ctx, prog)
		if err != nil {
			r.Logger.Error(err, "failed to set remove bpfProgram Finalizer")
			return ctrl.Result{Requeue: true, RequeueAfter: retryDurationOperator}, nil
		}
	}

	return ctrl.Result{}, nil
}

func (r *ReconcilerCommon) addFinalizer(ctx context.Context, prog client.Object, finalizer string) (ctrl.Result, error) {
	controllerutil.AddFinalizer(prog, bpfdOperatorFinalizer)

	err := r.Update(ctx, prog)
	if err != nil {
		r.Logger.V(1).Info("failed adding bpfd-operator finalizer to BpfProgramConfig...requeuing")
		return ctrl.Result{Requeue: true, RequeueAfter: retryDurationOperator}, nil
	}

	return ctrl.Result{}, nil
}

// Only reconcile if a bpfprogram object's status has been updated.
func statusChangedPredicate() predicate.Funcs {
	return predicate.Funcs{
		GenericFunc: func(e event.GenericEvent) bool {
			return false
		},
		CreateFunc: func(e event.CreateEvent) bool {
			return false
		},
		UpdateFunc: func(e event.UpdateEvent) bool {
			oldObject := e.ObjectOld.(*bpfdiov1alpha1.BpfProgram)
			newObject := e.ObjectNew.(*bpfdiov1alpha1.BpfProgram)
			return !reflect.DeepEqual(oldObject.Status, newObject.Status)
		},
		DeleteFunc: func(e event.DeleteEvent) bool {
			return false
		},
	}
}
